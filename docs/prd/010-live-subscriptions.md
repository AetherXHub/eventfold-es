# PRD 010: Live Subscriptions

**Status:** DRAFT
**Created:** 2026-02-27
**Author:** PRD Writer Agent

---

## Problem Statement

`eventfold-es` projections and process managers are pull-based: every call to
`store.projection::<P>()` opens a `SubscribeAll` gRPC stream, drains events
until the `CaughtUp` sentinel, closes the stream, and returns the state. Every
call to `store.run_process_managers()` does the same. This means:

1. **Read latency**: every projection read pays a full gRPC round-trip plus
   event replay from the last checkpoint, even when there are zero new events.
2. **Manual orchestration**: consumers must explicitly call
   `run_process_managers()` after every write that could trigger a reaction.
   Forgetting a call site means process managers silently fall behind.
3. **Wasted server resources**: the gRPC `SubscribeAll` stream supports a
   persistent live tail after `CaughtUp`, but the runners drop the stream
   immediately, forcing a new connection on every catch-up.

The underlying `eventfold-db` server already pushes new events on an open
`SubscribeAll` stream after the `CaughtUp` sentinel. The infrastructure for
live subscriptions exists at the transport layer; it is just not used.

## Goals

- Add a **live mode** to `ProjectionRunner` and `ProcessManagerRunner` that
  holds the `SubscribeAll` stream open past `CaughtUp` and continues to
  apply/react to events as the server pushes them.
- Expose live mode through `AggregateStore` so consumers can start background
  subscription tasks and read always-current projection state without catch-up
  latency.
- Make checkpoint flush frequency and reconnection backoff configurable via
  the existing `AggregateStoreBuilder` pattern.
- Keep the existing pull-based `store.projection::<P>()` and
  `store.run_process_managers()` unchanged and fully functional.

## Non-Goals

- Adding `SubscribeStream` (per-stream subscription) support. The global
  `SubscribeAll` stream is sufficient for projections and process managers,
  which already filter by `aggregate_type`/`event_type` in their method bodies.
  `SubscribeStream` can be added later as a performance optimization without
  changing the public API.
- Exposing raw `tonic::Streaming` handles to library consumers. The subscription
  lifecycle is managed internally.
- Distributed consensus or multi-node leader election for live subscriptions.
- Replacing the pull-based API. Both modes coexist.
- TLS or authentication on the gRPC channel (out of scope, same as PRD 009).

## User Stories

- As a Tauri desktop app developer (e.g., eventfold-crm), I want to call
  `store.start_live()` at startup and then read projections with
  `store.live_projection::<P>()` that return instantly without catch-up
  latency, so my UI is snappy even with many projections.

- As a developer with a `ProcessManager`, I want it to run continuously in the
  background so I don't have to remember to call `run_process_managers()` after
  every write, reducing bugs from missed call sites.

- As a library consumer, I want to configure how often checkpoints are saved
  and how the client reconnects on stream drops, so I can tune durability vs.
  I/O trade-offs for my use case.

- As a library consumer, I want to shut down live subscriptions gracefully
  (saving a final checkpoint) before my application exits.

## Technical Approach

### Overview

Live mode works by spawning a single background tokio task per
`AggregateStore` that holds a `SubscribeAll` stream open. This task is the
**live subscription loop**. It receives events, fans them out to all registered
projections and process managers, checkpoints periodically, and reconnects on
stream errors.

A single shared subscription is used rather than one per projection/PM because:
- All projections and process managers consume the same global log.
- The `SubscribeAll` stream delivers events in global-position order.
- A single stream means a single cursor, single checkpoint, and single
  reconnect lifecycle to manage.

### LiveConfig

```rust
/// Configuration for live subscription behavior.
///
/// All fields have sensible defaults. Pass to
/// [`AggregateStoreBuilder::live_config`] to customize.
#[derive(Debug, Clone)]
pub struct LiveConfig {
    /// How often to flush projection and process manager checkpoints to
    /// disk while in live mode.
    ///
    /// Checkpoints are also saved on graceful shutdown and when the stream
    /// reconnects. A shorter interval reduces replay on crash; a longer
    /// interval reduces disk I/O.
    ///
    /// Default: 5 seconds.
    pub checkpoint_interval: Duration,

    /// Base delay for exponential backoff on stream reconnection.
    ///
    /// After a stream error, the loop waits `reconnect_base_delay`,
    /// then `2 * reconnect_base_delay`, etc., up to `reconnect_max_delay`.
    /// A successful `CaughtUp` resets the backoff.
    ///
    /// Default: 1 second.
    pub reconnect_base_delay: Duration,

    /// Maximum delay between reconnection attempts.
    ///
    /// Default: 30 seconds.
    pub reconnect_max_delay: Duration,
}
```

### Builder integration

```rust
let store = AggregateStoreBuilder::new()
    .endpoint("http://127.0.0.1:2113")
    .base_dir(&data_dir)
    .projection::<PipelineSummary>()
    .projection::<ActivityFeed>()
    .process_manager::<DealTaskCreator>()
    .aggregate_type::<Deal>()
    .aggregate_type::<Task>()
    .live_config(LiveConfig {
        checkpoint_interval: Duration::from_secs(10),
        ..LiveConfig::default()
    })
    .open()
    .await?;
```

`live_config()` stores the config on the builder. If not called,
`LiveConfig::default()` is used. The config is stored on `AggregateStore` but
live mode is **not** started automatically at `open()` time -- the consumer
must explicitly call `start_live()`.

### AggregateStore API additions

```rust
impl AggregateStore {
    /// Start the live subscription loop in the background.
    ///
    /// Spawns a tokio task that subscribes to the global event log and
    /// continuously updates all registered projections and process
    /// managers. Returns a `LiveHandle` for reading projection state
    /// and shutting down.
    ///
    /// Can only be called once. Returns `Err` if already started.
    pub async fn start_live(&self) -> io::Result<LiveHandle>;

    /// Read a projection's state from the live subscription.
    ///
    /// Returns the projection's current state without triggering a
    /// catch-up. The state reflects all events processed by the live
    /// loop up to this point.
    ///
    /// Falls back to a pull-based catch-up if live mode is not started.
    pub async fn live_projection<P: Projection>(&self) -> io::Result<P>;
}
```

### LiveHandle

```rust
/// Handle for controlling the live subscription loop.
///
/// Dropping the handle does NOT stop the loop (it is owned by the
/// `AggregateStore`). Call `shutdown()` for graceful termination.
#[derive(Clone)]
pub struct LiveHandle { /* ... */ }

impl LiveHandle {
    /// Signal the live loop to stop and wait for a final checkpoint.
    ///
    /// Returns after the loop has saved all checkpoints and exited.
    pub async fn shutdown(&self) -> io::Result<()>;

    /// Returns true if the live loop has received a `CaughtUp` at
    /// least once (historical replay is complete).
    pub fn is_caught_up(&self) -> bool;
}
```

### Live subscription loop (internal)

The loop runs as a single tokio task and follows this lifecycle:

```
[start] --> subscribe_all_from(max_global_position)
              |
              v
         +--------+
   +---->| read   |----> event ----> fan-out to projections + PMs
   |     | stream |                  (apply / react)
   |     +--------+                       |
   |         |                            v
   |    CaughtUp -----> set caught_up     dispatch PM envelopes
   |         |          flag              (via store dispatchers)
   |         |                            |
   |         v                            v
   |     continue reading           checkpoint timer elapsed?
   |     (live tail)                 --> save all checkpoints
   |         |
   |    stream error / EOF
   |         |
   |         v
   |     save checkpoints
   |     exponential backoff wait
   |         |
   +----<----+  (reconnect)
```

Key implementation details:

1. **Fan-out**: The loop iterates all registered projections and PMs
   sequentially for each event. This is fast because `apply`/`react` are
   pure in-memory operations. Projection state is held behind a
   `tokio::sync::RwLock` so `live_projection` reads can proceed
   concurrently with event application.

2. **Global cursor**: The loop tracks a single `last_global_position` cursor.
   On reconnect, it subscribes from `max(all projection positions, all PM
   positions)` to avoid replaying events that have already been processed.
   This is identical to the current catch-up behavior.

3. **Checkpoint flushing**: A `tokio::time::interval` fires every
   `checkpoint_interval`. When it fires, all projection and PM checkpoints
   are saved to disk. Checkpoints are also saved on shutdown and before
   reconnect.

4. **Process manager dispatch**: Command envelopes produced by PMs during
   live mode are dispatched immediately through the existing
   `AggregateStore` dispatcher infrastructure. Dead-lettering on failure
   uses the existing `append_dead_letter` mechanism.

5. **PM checkpoint deferred save**: Consistent with the existing
   `ProcessManagerRunner` contract, PM checkpoints are saved only after
   envelopes have been dispatched (or dead-lettered), not before.

6. **Reconnection**: On stream error, the loop saves checkpoints, waits
   with exponential backoff (capped at `reconnect_max_delay`), and
   reconnects. A successful `CaughtUp` resets the backoff counter.

7. **Shutdown**: A `tokio::sync::watch` channel signals the loop to exit.
   On receiving the signal, the loop saves final checkpoints and returns.

### Projection state access in live mode

Currently, `ProjectionRunner` holds the projection state inside a
`tokio::sync::Mutex`. For live mode, projections need concurrent read access
(from `live_projection` callers) while the live loop writes.

The live loop holds a **write lock** briefly while applying each event, then
releases it. `live_projection` reads acquire a **read lock** and clone the
state. Since `Projection: Clone`, this is always possible.

To support this, the internal `ProjectionRunner` state storage changes from
being behind `tokio::sync::Mutex` to `tokio::sync::RwLock`. The pull-based
`store.projection::<P>()` path is updated accordingly (takes a write lock
for catch-up, same as today's exclusive mutex lock).

### Interaction with pull-based API

The pull-based `store.projection::<P>()` and `store.run_process_managers()`
remain fully functional regardless of whether live mode is active:

- **Live mode active**: `store.projection::<P>()` skips the catch-up (the
  live loop is already keeping the projection current) and returns the
  current state. This is equivalent to `store.live_projection::<P>()`.
- **Live mode inactive**: `store.projection::<P>()` works exactly as today
  (catch-up then return).
- `store.run_process_managers()` always performs a catch-up pass regardless
  of live mode, ensuring it can be used as a manual trigger if needed.

### Files changed

| File | Action | Summary |
|------|--------|---------|
| `src/live.rs` | Create | `LiveConfig`, `LiveHandle`, live subscription loop |
| `src/store.rs` | Modify | Add `live_config` field, `start_live()`, `live_projection()` to `AggregateStore`; add `live_config()` to builder |
| `src/projection.rs` | Modify | Change internal state lock from `Mutex` to `RwLock`; add `state()` read accessor for live mode |
| `src/process_manager.rs` | Modify | Add read accessor for live mode state |
| `src/lib.rs` | Modify | Re-export `LiveConfig`, `LiveHandle` |

### Consumer migration example (eventfold-crm)

Before (pull-based):
```rust
// Setup
let store = AggregateStoreBuilder::new()
    .endpoint(&endpoint)
    .projection::<PipelineSummary>()
    .process_manager::<DealTaskCreator>()
    .aggregate_type::<Deal>()
    .open()
    .await?;

// Every read (catch-up on every call)
let summary = store.projection::<PipelineSummary>().await?;

// After every write (manual trigger)
dispatch::<Deal>(&store, &id, cmd, ctx()).await?;
store.run_process_managers().await?;
```

After (live mode):
```rust
// Setup
let store = AggregateStoreBuilder::new()
    .endpoint(&endpoint)
    .projection::<PipelineSummary>()
    .process_manager::<DealTaskCreator>()
    .aggregate_type::<Deal>()
    .open()
    .await?;
let live = store.start_live().await?;

// Every read (instant, no catch-up)
let summary = store.live_projection::<PipelineSummary>().await?;

// After writes (no manual trigger needed)
dispatch::<Deal>(&store, &id, cmd, ctx()).await?;
// process managers run automatically in the background

// On app shutdown
live.shutdown().await?;
```

## Acceptance Criteria

1. `store.start_live().await` returns `Ok(handle)` when a `SubscribeAll` stream
   can be established with the server.

2. `store.start_live().await` returns `Err` when called a second time on the
   same `AggregateStore` instance.

3. After `start_live()`, `handle.is_caught_up()` returns `true` once the live
   loop has received the initial `CaughtUp` sentinel.

4. After `start_live()` and `is_caught_up() == true`,
   `store.live_projection::<P>()` returns state reflecting all events that
   existed at the time of the `CaughtUp` sentinel.

5. After `start_live()`, appending a new event via `handle.execute()` and
   waiting briefly, `store.live_projection::<P>()` reflects the new event
   without the consumer calling `store.projection::<P>()` or any manual
   catch-up.

6. After `start_live()`, process manager command envelopes are dispatched
   automatically when matching events are appended. No manual
   `store.run_process_managers()` call is required.

7. Dead-lettered envelopes during live mode are written to the same
   per-PM dead-letter log files as in pull mode.

8. `handle.shutdown().await` returns `Ok(())`, the background task exits,
   and all projection/PM checkpoints are saved to disk. After shutdown,
   `store.projection::<P>()` (pull mode) returns state consistent with what
   the live loop last processed.

9. After a stream disconnection (server restart), the live loop reconnects
   with exponential backoff and resumes from the last checkpointed position
   without losing or duplicating events.

10. `LiveConfig::default()` provides `checkpoint_interval = 5s`,
    `reconnect_base_delay = 1s`, `reconnect_max_delay = 30s`.

11. `AggregateStoreBuilder::new().live_config(custom_config)` stores the
    config and it is used by `start_live()`.

12. The pull-based `store.projection::<P>()` continues to work identically
    whether or not live mode is active.

13. `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` all
    pass.

14. All new public types (`LiveConfig`, `LiveHandle`) and methods
    (`start_live`, `live_projection`, `shutdown`, `is_caught_up`) have doc
    comments following the project's established pattern.

## Open Questions

- **Should `live_projection` return a `watch::Receiver` instead of a clone?**
  A `tokio::sync::watch` channel would let consumers `.await` the next state
  change (reactive reads). This PRD specifies clone-based reads for simplicity;
  a watch channel can be layered on in a follow-up without changing the live
  loop internals.

- **Should `start_live` perform the initial catch-up synchronously before
  returning?** This PRD specifies that `start_live` returns immediately after
  spawning the background task, and consumers check `is_caught_up()` or wait.
  An alternative is to block until the first `CaughtUp` so that
  `live_projection` is guaranteed valid immediately after `start_live` returns.

- **Should the live loop run projections and PMs in parallel (per event)?**
  This PRD specifies sequential fan-out for simplicity and determinism.
  Parallelism is a follow-on optimization if fan-out becomes a bottleneck.

## Dependencies

No new crate dependencies are required. All needed primitives already exist:

- `tokio::time::interval` for checkpoint flushing (already in `tokio` dep).
- `tokio::sync::watch` for shutdown signaling (already in `tokio` dep).
- `tokio::sync::RwLock` for concurrent projection reads (already in `tokio` dep).
- `EsClient::subscribe_all_from` for the gRPC stream (already implemented).
- `ProjectionRunner`, `ProcessManagerRunner`, checkpoint persistence, and
  dead-letter logging are all existing internal infrastructure.
