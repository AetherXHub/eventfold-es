# PRD 012: LiveHandle Position Notification Channel

**Status:** TICKETS READY
**Created:** 2026-02-28
**Author:** PRD Writer Agent
**GitHub Issue:** #6

---

## Problem Statement

The live subscription loop in `eventfold-es` processes events from the gRPC
stream and fans them out to projections and process managers, but it provides no
signal to application code when that processing has occurred. Applications that
need to react to newly processed events — for example, a Tauri desktop app
invalidating a query cache after a remote instance writes an event — must resort
to polling `live_projection()`, which is wasteful and adds latency. A zero-cost
coalescing notification channel on `LiveHandle` eliminates the polling
requirement without coupling the library to any particular application framework.

## Goals

- Add a `pub fn subscribe(&self) -> watch::Receiver<u64>` method to
  `LiveHandle` that returns a receiver whose value is the most recently
  processed global position.
- Ensure the `watch::Sender` is created unconditionally inside `start_live()`
  and stored in `LiveHandle`, adding zero overhead when `subscribe()` is never
  called.
- Send the current `recorded.global_position` through the channel after each
  event is fully fanned out to projections and process managers (including PM
  dispatch) in `process_stream_with_dispatch`.
- Preserve full backward compatibility: no existing public method signatures on
  `LiveHandle`, `AggregateStore`, or any other type change.

## Non-Goals

- Sending individual event payloads or event types through the channel (the
  gRPC subscription stream is the right API for that).
- A callback-based notification API.
- Exposing the raw gRPC `SubscribeResponse` stream to callers.
- Per-projection or per-aggregate-type position channels.
- Cross-process notification (e.g., IPC, OS signals).
- Bridging this channel to any specific frontend framework (Tauri event system,
  web sockets, etc.) — that is application code's responsibility.

## User Stories

- As a Tauri application developer, I want to call `handle.subscribe()` and
  `await receiver.changed()` in a background task, so that I can emit a Tauri
  event to the frontend immediately after new events are processed without
  polling.
- As a library consumer, I want `subscribe()` to be callable multiple times
  and from multiple clones of `LiveHandle`, so that independent subsystems can
  each hold their own `watch::Receiver` without coordinating.
- As a library consumer who does not need change notifications, I want the
  absence of any `subscribe()` call to have no measurable runtime cost, so that
  the channel does not degrade throughput for applications that do not use it.

## Technical Approach

### Affected files

| File | Change |
|------|--------|
| `src/live.rs` | Add `position_tx` field to `LiveHandle`; add `subscribe()` method; update `process_stream_with_dispatch` signature to accept `&watch::Sender<u64>` and send position after each event; update `run_live_loop` to pass the sender through |
| `src/store.rs` | Initialize `watch::channel(0u64)` in `start_live()` and populate the new field in the `LiveHandle` literal |

### `LiveHandle` struct change

Add one field to the existing struct in `src/live.rs`:

```rust
pub struct LiveHandle {
    pub(crate) shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub(crate) caught_up: Arc<AtomicBool>,
    pub(crate) task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<io::Result<()>>>>>,
    /// Carries the latest global position after each event is processed.
    /// Initialized to 0. Callers obtain a receiver via [`subscribe`](LiveHandle::subscribe).
    pub(crate) position_tx: Arc<tokio::sync::watch::Sender<u64>>,
}
```

`Arc`-wrapping the sender matches the existing pattern for `shutdown_tx` fields
on cloneable handles and ensures `Clone` remains `#[derive]`-able without
requiring a manual impl.

### New public method

```rust
/// Subscribe to global position updates from the live loop.
///
/// Returns a `watch::Receiver<u64>` whose value is updated to the most
/// recently processed event's global position after each event is fully
/// fanned out to all projections and process managers. The initial value
/// is `0`.
///
/// # Usage
///
/// ```no_run
/// let mut rx = handle.subscribe();
/// loop {
///     rx.changed().await.expect("live loop dropped");
///     let pos = *rx.borrow_and_update();
///     // react to pos
/// }
/// ```
///
/// Multiple receivers can be created: each call to `subscribe()` returns an
/// independent receiver, and cloning an existing `watch::Receiver` also
/// works. All receivers see the same coalesced position value.
pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
    self.position_tx.subscribe()
}
```

`watch::Sender::subscribe()` is the idiomatic way to create additional
receivers from an existing sender; it is already available in the `tokio`
version in use.

### `process_stream_with_dispatch` signature change

Add `position_tx: &tokio::sync::watch::Sender<u64>` as a parameter. This is a
`pub(crate)` function, so the change is non-breaking to external consumers.

After the PM dispatch loop for each event (the last action in the
`subscribe_response::Content::Event` arm), add:

```rust
// Notify subscribers of the newly processed position. `send` on a
// watch channel only fails if all receivers have been dropped, which
// is not an error condition -- the `let _` discards that case.
let _ = position_tx.send(recorded.global_position);
```

The send happens after projections are updated and PM envelopes are dispatched,
so receivers see the position only once all processing for that event is
complete.

### `run_live_loop` threading

`run_live_loop` already receives `caught_up: Arc<AtomicBool>` as a parameter.
Add `position_tx: Arc<tokio::sync::watch::Sender<u64>>` in the same fashion,
and pass a reference (`&position_tx`) to each `process_stream_with_dispatch`
call. No `Arc` clone is needed at the call site since the function only borrows
the sender.

### Initialization in `start_live()` (`src/store.rs`)

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
let (position_tx, _position_rx) = tokio::sync::watch::channel(0u64);
let position_tx = Arc::new(position_tx);
let caught_up = Arc::new(AtomicBool::new(false));

// pass Arc::clone(&position_tx) to the spawned task
let handle = LiveHandle {
    shutdown_tx,
    caught_up,
    task: Arc::new(tokio::sync::Mutex::new(Some(task))),
    position_tx,
};
```

The initial `_position_rx` from `watch::channel` is discarded immediately;
callers obtain receivers via `handle.subscribe()`. Discarding it does not close
the channel — the `Sender` remains alive inside `LiveHandle`.

### Test changes

Existing tests that construct `LiveHandle` directly (in `src/live.rs` tests)
must add the `position_tx` field. Add it using
`Arc::new(tokio::sync::watch::channel(0u64).0)` in each bare struct literal.

Add new unit tests (inline in `src/live.rs`):

1. `subscribe_returns_receiver_with_initial_value_zero` — construct a bare
   `LiveHandle`, call `subscribe()`, assert `*rx.borrow() == 0`.
2. `subscribe_multiple_times_returns_independent_receivers` — call `subscribe()`
   twice, assert both start at 0 and both see the same updated value after a
   simulated send.
3. `process_stream_sends_position_after_each_event` — run
   `process_stream_with_dispatch` with a two-event stream, then assert the
   `watch` sender's current value equals the `global_position` of the second
   event (`1`).
4. `subscribe_on_cloned_handle_shares_same_channel` — clone a `LiveHandle`,
   call `subscribe()` on the clone, send a position from the original sender,
   assert the cloned receiver sees the update (i.e., `Arc::ptr_eq` holds and
   both ends share the same channel).

## Acceptance Criteria

1. `LiveHandle` has a `pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64>` method and compiles with `cargo build --locked`.
2. `handle.subscribe()` called before any events are processed returns a receiver whose initial `*rx.borrow()` value is `0`.
3. After `process_stream_with_dispatch` processes N events, `*rx.borrow_and_update()` equals the `global_position` field of the Nth (last) event.
4. Calling `subscribe()` on two different clones of the same `LiveHandle` yields two receivers that both observe the same position updates (verified via `Arc::ptr_eq` on the underlying `position_tx`).
5. Calling `subscribe()` multiple times on the same `LiveHandle` instance yields independent `watch::Receiver` values that each observe the same position updates.
6. Existing tests in `src/live.rs` and `src/store.rs` pass without modification to their assertions (only the `LiveHandle` literal construction lines may change to add the new field).
7. `cargo clippy --all-targets --all-features --locked -- -D warnings` exits with code 0.
8. `cargo test --locked` exits with all tests green, including the four new tests listed in the Technical Approach.
9. No existing public method on `LiveHandle` or `AggregateStore` changes its signature.
10. The `watch::Sender<u64>` is constructed unconditionally inside `start_live()` regardless of whether `subscribe()` is ever called.

## Open Questions

- Should the initial channel value be `0` (position-before-any-event) or
  `u64::MAX` as a sentinel for "nothing yet processed"? Default decision: `0`,
  matching the global position semantics used throughout the codebase
  (`min_global_position` returns `0` when no projections exist). Receivers can
  distinguish "no events seen yet" vs "events processed" by comparing the
  received value against their last known position or by using `changed()` to
  wait for the first update.
- Should the `CaughtUp` sentinel also trigger a channel send? It carries no
  `global_position`, so there is nothing meaningful to send. Default decision:
  no — only `RecordedEvent` messages send a position update. If callers need to
  know when catch-up completes, they use `is_caught_up()`.

## Dependencies

- `tokio::sync::watch` is already used in `src/live.rs` for `shutdown_tx`; no
  new Cargo dependencies are required.
- This feature is purely additive and does not depend on any other open PRDs.
- Consumers of the notification channel (e.g., eventfold-crm Tauri integration)
  depend on this PRD being implemented first.
