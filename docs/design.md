# eventfold-es: High-Level Design

> Embedded CQRS/Event Sourcing built on `eventfold`.

## 1. Vision

`eventfold-es` is a zero-infrastructure, embedded CQRS/ES runtime for Rust. It maps
Domain-Driven Design aggregates onto `eventfold` event logs -- one log per aggregate
instance -- and provides async orchestration, cross-stream projections, and process
managers on top.

The target audience is the same as `eventfold`: developers who want event sourcing
without deploying a database, message broker, or event store. Think single-binary CLIs,
local-first desktop apps, embedded devices, and prototypes that may later graduate to a
distributed event store.

### Goals

- **Embedded**: no network services, no external databases. Just files on disk.
- **Async-first**: all public APIs are `async`, backed by `tokio`.
- **Type-safe aggregates**: command handling, event application, and state projection
  are expressed through Rust traits with compile-time guarantees.
- **Multi-stream**: each aggregate instance owns its own `eventfold` log, enabling
  independent append, compaction, and snapshot lifecycles.
- **Cross-stream projections**: read models that fold events from many streams into a
  single queryable view.
- **Process managers**: react to events in one aggregate by dispatching commands to
  another, enabling long-running workflows.
- **Minimal overhead**: thin orchestration layer; the heavy lifting stays in `eventfold`.

### Non-goals

- Distributed or multi-node operation.
- Network transport or RPC protocols.
- A full-featured query language for read models.
- Schema migration tooling (for now; see "Future: Event Versioning" in open questions).

---

## 2. Core Concepts

| DDD / CQRS / ES concept | eventfold-es mapping |
|---|---|
| Aggregate | A Rust type implementing the `Aggregate` trait |
| Aggregate instance | A single `eventfold::EventLog` in its own directory |
| Command | An enum variant validated and executed by the aggregate |
| Domain event | An enum variant serialized as an `eventfold::Event` |
| Aggregate state | An `eventfold::View` whose reducer applies domain events |
| Event stream | The `app.jsonl` file inside an aggregate instance's directory |
| Projection / Read model | A `Projection` that folds events across many streams |
| Process manager | A `ProcessManager` that reacts to events with commands |
| Aggregate store | The `AggregateStore` that manages directories and caching |

---

## 3. Architecture Overview

```
                          Command
                            |
                            v
                    +---------------+
                    | CommandBus    |  (async dispatch, routing)
                    +-------+-------+
                            |
                            v
                    +---------------+
                    | AggregateStore|  (stream lifecycle, caching)
                    +-------+-------+
                            |
            +---------------+---------------+
            |               |               |
            v               v               v
     +-----------+   +-----------+   +-----------+
     | EventLog  |   | EventLog  |   | EventLog  |   (one per aggregate instance)
     | (order-1) |   | (order-2) |   | (user-7)  |
     +-----------+   +-----------+   +-----------+
            |               |               |
            +-------+-------+-------+-------+
                    |               |
                    v               v
            +-------------+  +-----------------+
            | Projections |  | ProcessManagers |  (cross-stream consumers)
            +-------------+  +-----------------+
                    |
                    v
              Read Models
              (query side)
```

### Data flow

1. A **command** enters the `CommandBus`.
2. The bus resolves the target aggregate type and instance ID.
3. The `AggregateStore` loads or creates the aggregate instance's `EventLog`.
4. The aggregate's current state is refreshed (`View::refresh`).
5. The `Aggregate::handle` method validates the command against current state and
   returns zero or more domain events.
6. Events are appended to the instance's log via `EventWriter::append_if`
   (optimistic concurrency: the state we validated against must still be current).
7. On conflict, the cycle retries: reload state, re-validate, re-append.
8. After successful append, newly written events are dispatched to registered
   `Projection`s and `ProcessManager`s.

---

## 4. Storage Layout

```
<base_dir>/
    streams/
        <aggregate_type>/               # e.g. "order", "user"
            <instance_id>/              # e.g. "ord-001", "usr-42"
                app.jsonl               # eventfold active log
                archive.jsonl.zst       # eventfold archive
                views/
                    state.snapshot.json  # aggregate state snapshot
    projections/
        <projection_name>/              # e.g. "order-summary"
            state.json                  # projection state + cursor metadata
    meta/
        streams.jsonl                   # stream registry (type, id, created_at)
```

- Each aggregate instance is a standard `eventfold` log directory, fully compatible
  with standalone `eventfold` tooling.
- The `streams/` subtree can be inspected, backed up, or replayed with plain `eventfold`
  APIs.
- The `projections/` subtree stores cross-stream read model state and the cursors that
  track which streams/offsets have been consumed.
- The `meta/streams.jsonl` file is itself an append-only log that records stream
  creation and deletion, enabling discovery without a filesystem scan.

---

## 5. Key Traits

The following are design-level sketches, not final API signatures.

### 5.1 Aggregate

```rust
/// A domain aggregate whose state is derived from its event history.
///
/// Type parameters:
/// - The implementing type itself serves as the aggregate's state.
///
/// Associated types:
/// - `Command`: the set of commands this aggregate can handle.
/// - `DomainEvent`: the set of events this aggregate can produce and apply.
/// - `Error`: command rejection / validation error.
pub trait Aggregate: Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Identifies this aggregate type (e.g. "order"). Used as a directory name.
    const AGGREGATE_TYPE: &'static str;

    type Command: Send + 'static;
    type DomainEvent: Serialize + DeserializeOwned + Send + Sync + Clone + 'static;
    type Error: std::error::Error + Send + Sync + 'static;

    /// Validate a command against the current state and produce events.
    ///
    /// Must be a pure decision: no I/O, no side effects.
    /// Returns `Ok(vec![])` if the command is a no-op.
    /// Returns `Err` to reject the command.
    fn handle(&self, cmd: Self::Command) -> Result<Vec<Self::DomainEvent>, Self::Error>;

    /// Apply a single event to produce the next state.
    ///
    /// Must be a pure, total function. Unknown event variants should be
    /// ignored (forward compatibility).
    fn apply(self, event: &Self::DomainEvent) -> Self;
}
```

The `apply` method mirrors `eventfold::ReduceFn<S>` semantics: it takes ownership of
state and returns the next state. Under the hood, `eventfold-es` generates a
`ReduceFn<S>` adapter that deserializes the `eventfold::Event` payload into
`DomainEvent` and delegates to `apply`.

### 5.1.1 Event metadata mapping

`eventfold::Event` carries optional `id`, `actor`, and `meta` fields (added in v0.2.0).
`eventfold-es` populates these automatically when appending domain events:

| `Event` field | Source | Purpose |
|---|---|---|
| `event_type` | `DomainEvent` variant name (via serde tag) | Stream routing, reducer dispatch |
| `data` | Serialized `DomainEvent` payload | Domain payload |
| `id` | Caller-supplied or auto-generated | Idempotency (dedup on replay) |
| `actor` | From `CommandContext` (see below) | Audit trail |
| `meta` | From `CommandContext` | Correlation IDs, session, causation |

A `CommandContext` is passed alongside the command to carry cross-cutting metadata
without polluting the `Command` or `DomainEvent` types:

```rust
pub struct CommandContext {
    /// Identity of the actor issuing the command.
    pub actor: Option<String>,
    /// Correlation ID for tracing a request across aggregates.
    pub correlation_id: Option<String>,
    /// Arbitrary metadata forwarded to `Event::meta`.
    pub metadata: Option<Value>,
}
```

### 5.2 Projection

```rust
/// A cross-stream read model that consumes events from multiple aggregate streams.
pub trait Projection: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Human-readable name, used as a directory name under `projections/`.
    const NAME: &'static str;

    /// The set of aggregate types this projection subscribes to.
    /// Each entry is an `AGGREGATE_TYPE` string.
    fn subscriptions(&self) -> &[&str];

    /// Apply a single event from any subscribed stream.
    ///
    /// `stream_id` identifies the aggregate instance (e.g. "ord-001").
    /// `event` is the raw `eventfold::Event`; the projection decides
    /// how to interpret it based on `event.event_type`.
    fn apply(&mut self, aggregate_type: &str, stream_id: &str, event: &Event);
}
```

Projections are **eventually consistent**: they catch up by polling streams and are
rebuilt from scratch if their checkpoint is invalid.

### 5.3 ProcessManager

```rust
/// Reacts to events by producing commands for other aggregates.
///
/// Used for cross-aggregate workflows (e.g. "when an order is placed,
/// reserve inventory").
pub trait ProcessManager: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;

    fn subscriptions(&self) -> &[&str];

    /// React to an event. Returns zero or more commands to dispatch.
    ///
    /// Each `CommandEnvelope` specifies the target aggregate type,
    /// instance ID, and the serialized command payload.
    fn react(
        &mut self,
        aggregate_type: &str,
        stream_id: &str,
        event: &Event,
    ) -> Vec<CommandEnvelope>;
}
```

Process managers maintain their own persisted state and checkpoint, similar to
projections, but produce side effects (commands) rather than read models.

---

## 6. Async Model

`eventfold` v0.2.0 separates reading and writing into independent types with favorable
thread-safety properties:

| Type | Send | Sync | Clone | Notes |
|---|---|---|---|---|
| `EventWriter` | yes | yes | no | Exclusive writer, holds flock |
| `EventReader` | yes | yes | yes | Cheap, opens fresh handles per read |
| `EventLog` | **no** | **no** | no | Composite; `!Send` due to `Box<dyn ViewOps>` |
| `View<S>` | if S: Send | if S: Sync | no | Takes `&EventReader` for refresh |

Because `EventWriter` and `EventReader` are both `Send + Sync`, and `View<S>` takes
`&EventReader` (not `&EventLog`), `eventfold-es` can bypass `EventLog` entirely and
compose these primitives directly.

`eventfold-es` uses a **dedicated-thread actor model** per open aggregate instance.
Each actor owns an `EventWriter` and a `View<AggregateState>` -- not an `EventLog`.
`EventReader` clones are shared freely into the async world for projections and queries.

```
                    async world (tokio)
                    ===================

    CommandBus ---msg---> AggregateHandle ---channel---> AggregateActor
                          (Clone, Send)                  (owns EventWriter +
                                                          View<S>, runs on
                                                          dedicated thread)

    Projections --------> EventReader (cloned per projection, freely Send + Sync)
```

### AggregateHandle

A lightweight, cloneable, `Send + Sync` handle that communicates with the actor
over a `tokio::sync::mpsc` channel. Public methods are async and return a
`tokio::sync::oneshot` result.

```rust
impl<A: Aggregate> AggregateHandle<A> {
    /// Send a command and wait for the result.
    pub async fn execute(
        &self,
        cmd: A::Command,
        ctx: CommandContext,
    ) -> Result<Vec<A::DomainEvent>, ExecuteError<A>>;

    /// Read the current aggregate state (eventually consistent).
    pub async fn state(&self) -> Result<A, StateError>;
}
```

### AggregateActor

Runs in a dedicated blocking context. Owns the `EventWriter` and aggregate state
`View<A>`. Processes messages sequentially, ensuring single-writer semantics without
external locking beyond what `eventfold` already provides.

The actor loop:

1. Receive a message `(cmd, ctx, reply_tx)` from the channel.
2. Refresh the aggregate state view via `view.refresh(&reader)`.
3. Execute the command: `aggregate.handle(cmd)`.
4. Convert each `DomainEvent` to an `eventfold::Event`, populating `actor`, `meta`,
   and `id` from the `CommandContext`.
5. Append resulting events with `writer.append_if` (optimistic concurrency).
6. On conflict, retry from step 2 (up to a configurable limit).
7. Send the result back via the oneshot.
8. Notify the event dispatcher of new events.

### Projections and readers

Because `EventReader` is `Clone + Send + Sync`, projections run entirely in the async
world. Each projection holds its own `EventReader` clones for the streams it subscribes
to and catches up independently -- no need to route reads through the aggregate actor.
This decouples read-side latency from write-side command processing.

`EventReader::has_new_events` provides cheap polling; `wait_for_events` provides
OS-level file-watch blocking (wrapped in `spawn_blocking` when used from async).

### Idle eviction

Aggregate actors that have been idle beyond a configurable timeout are shut down and
their resources (file handles, memory) released. The `AggregateStore` transparently
re-spawns them on the next command or query.

---

## 7. AggregateStore

The `AggregateStore` is the central registry that manages aggregate instance lifecycles.

```rust
impl AggregateStore {
    /// Open or create a store rooted at `base_dir`.
    pub async fn open(base_dir: impl AsRef<Path>) -> Result<Self>;

    /// Get a handle to an aggregate instance, spawning its actor if needed.
    pub async fn get<A: Aggregate>(&self, id: &str) -> Result<AggregateHandle<A>>;

    /// List all instance IDs for a given aggregate type.
    pub async fn list<A: Aggregate>(&self) -> Result<Vec<String>>;

    /// Register a projection to be updated after each append.
    pub fn register_projection<P: Projection>(&mut self, projection: P);

    /// Register a process manager.
    pub fn register_process_manager<PM: ProcessManager>(&mut self, pm: PM);
}
```

Internally, the store maintains a `HashMap` of active `AggregateHandle`s keyed by
`(aggregate_type, instance_id)`. It is responsible for:

- Creating the directory structure for new instances.
- Recording stream creation in `meta/streams.jsonl`.
- Spawning and caching actors.
- Evicting idle actors.
- Driving projection and process manager catch-up loops.

---

## 8. Event Dispatch and Ordering

After an aggregate actor successfully appends events, it notifies the central
**EventDispatcher**. The dispatcher is responsible for:

1. **Projection catch-up**: for each registered projection, read new events from the
   affected stream and call `Projection::apply`. Projections track per-stream cursors
   (offsets + hashes) and persist them alongside their state.

2. **Process manager reactions**: for each registered process manager, deliver new
   events and collect any `CommandEnvelope`s. These commands are dispatched back through
   the `CommandBus`.

### Ordering guarantees

- **Within a single stream**: events are strictly ordered (guaranteed by `eventfold`'s
  append-only file and single-writer lock).
- **Across streams**: no global ordering. Projections and process managers observe
  events per-stream in order, but interleaving across streams is non-deterministic.
- **Projection consistency**: projections are eventually consistent. They may lag
  behind the write side by one or more events. `eventfold-es` does not provide
  read-after-write consistency for projections (see open questions).

---

## 9. Error Handling Strategy

| Error class | Handling |
|---|---|
| Command validation failure | Returned to caller as `Err(A::Error)` |
| Optimistic concurrency conflict | Automatic retry (configurable limit) |
| I/O error (disk full, permissions) | Propagated as `ExecuteError::Io` |
| Corrupt snapshot | Automatic rebuild (handled by `eventfold`) |
| Corrupt log (partial write) | Partial line skipped (handled by `eventfold`) |
| Actor channel closed | `ExecuteError::ActorGone` -- store re-spawns on next call |
| Projection failure | Logged, retried on next catch-up cycle |
| Process manager failure | Logged, dead-lettered for manual inspection |

---

## 10. Concurrency Boundaries

```
+----------------------------------------------------------+
|                  tokio async runtime                      |
|                                                          |
|  CommandBus        AggregateStore                        |
|  (all Send + Sync)                                       |
|                                                          |
|  Projections + ProcessManagers                           |
|  (own EventReader clones, run as async tasks)            |
+-----+-------------------+-------------------+------------+
      |                   |                   |
      v                   v                   v
+-------------+    +-------------+    +-------------+
| blocking    |    | blocking    |    | blocking    |
| thread      |    | thread      |    | thread      |
| Actor(1)    |    | Actor(2)    |    | Actor(N)    |
| EventWriter |    | EventWriter |    | EventWriter |
| View<S>     |    | View<S>     |    | View<S>     |
+-------------+    +-------------+    +-------------+
```

- Each actor thread owns an `EventWriter` + `View<S>` and processes commands
  sequentially. `EventLog` is not used directly.
- `EventReader` clones live in the async world. Projections and process managers
  use them to catch up on streams without involving the actor.
- `EventReader::wait_for_events` (blocking) is wrapped in `spawn_blocking` when
  called from async projection catch-up loops.
- The only cross-boundary communication is the `mpsc` channel between
  `AggregateHandle` (async) and `AggregateActor` (blocking thread).

---

## 11. Testing Strategy

### Unit tests

- Each `Aggregate` implementation is tested in isolation: given state + command,
  assert produced events or error. No I/O.
- Each `Aggregate::apply` is tested: given state + event, assert next state. No I/O.
- Projections tested similarly: given state + sequence of events, assert final read
  model.

### Integration tests

- `AggregateStore` round-trips: create instance, send commands, read state, verify
  events on disk.
- Concurrency tests: multiple handles sending commands to the same aggregate instance,
  verifying optimistic concurrency retries.
- Projection catch-up: append events, verify projections converge.
- Process manager workflows: trigger cross-aggregate flows end-to-end.

### Property-based tests

- Replaying the same event sequence always produces the same state.
- `handle` + `apply` round-trip: events produced by `handle` are accepted by `apply`.
- Snapshot rebuild produces the same state as fresh replay.

---

## 12. Dependency Budget

| Crate | Purpose |
|---|---|
| `eventfold` | Event log, views, snapshots |
| `tokio` | Async runtime, channels, spawn_blocking |
| `serde` + `serde_json` | Serialization (re-export from eventfold) |
| `thiserror` | Error types |
| `tracing` | Structured logging |

Optional / deferred:

| Crate | Purpose |
|---|---|
| `uuid` | Default ID generation for new aggregate instances |

The dependency budget is intentionally minimal. `eventfold` already brings in `serde`,
`serde_json`, `fs2`, `notify`, `xxhash-rust`, and `zstd`.

---

## 13. Crate Structure

```
eventfold-es/
    src/
        lib.rs              # Public API re-exports
        aggregate.rs        # Aggregate trait + ReduceFn adapter
        store.rs            # AggregateStore
        actor.rs            # AggregateActor + AggregateHandle
        command.rs          # CommandBus, CommandEnvelope
        projection.rs       # Projection trait + catch-up loop
        process_manager.rs  # ProcessManager trait + dispatch
        dispatcher.rs       # EventDispatcher (post-append fan-out)
        error.rs            # Error types
        storage.rs          # Directory layout, stream registry
    tests/
        integration/
            aggregate.rs
            projection.rs
            process_manager.rs
            concurrency.rs
```

The crate is a library (`lib` target). No binary. Users depend on `eventfold-es` and
compose their domain logic by implementing the traits.

---

## 14. Open Questions

These are decisions to be made during detailed design / PRD phase:

1. **Read-after-write consistency for projections**: should we offer a
   `store.execute_and_project()` that blocks until projections have consumed the new
   events? This simplifies application code but couples the write path to projection
   speed.

2. **Aggregate ID generation**: should the store auto-generate UUIDs for new instances,
   or require the caller to supply IDs? Auto-generation is convenient but opaque;
   caller-supplied IDs enable natural keys (e.g. `order-{order_number}`).

3. **Snapshot strategy for projections**: projections consume many streams. Should they
   snapshot per-stream cursors independently, or checkpoint atomically across all
   subscribed streams? Atomic checkpoints are simpler but require re-processing more
   data on partial failure.

4. **Event versioning**: as `DomainEvent` enums evolve over time, the system will need
   a strategy for reading old events. This is out of scope for Phase 1 (lean on
   `serde(default)` and additive-only changes for now), but the serialization format
   and trait design should avoid painting us into a corner. Future work may include
   schema-as-asset-class (persisted, versioned schema metadata alongside streams),
   upcasting hooks, and/or derive macros for migration. Worth revisiting once the
   core aggregate loop is stable and real evolution pressure is felt.

5. **Backpressure on process managers**: if a process manager produces commands faster
   than they can be executed, how do we handle backpressure? Bounded channel?
   In-memory buffer with spill-to-disk?

6. **Multi-aggregate transactions**: CQRS/ES traditionally avoids cross-aggregate
   transactions (use sagas instead). Should we explicitly forbid them, or provide a
   best-effort "append to N streams atomically" primitive? The latter is possible with
   filesystem rename tricks but adds complexity.

7. **Compaction / stream deletion**: should `eventfold-es` expose APIs for archiving
   or deleting aggregate instances? What happens to projections that reference deleted
   streams?

8. **Derive macro**: should we provide a `#[derive(Aggregate)]` macro to reduce
   boilerplate (auto-generating the `ReduceFn` adapter, event type
   serialization/deserialization, etc.)? This is a convenience feature that can be added
   later without breaking changes.

---

## 15. Phased Delivery

### Phase 1: Core aggregate loop

- `Aggregate` trait
- `AggregateStore` with actor-per-instance model
- `AggregateHandle` with `execute` and `state`
- Directory layout and stream registry
- Integration tests

### Phase 2: Projections

- `Projection` trait
- Cross-stream catch-up loop with per-stream cursors
- Projection persistence and rebuild
- Projection registration on `AggregateStore`

### Phase 3: Process managers

- `ProcessManager` trait
- `CommandEnvelope` and dispatch loop
- Dead-letter handling
- End-to-end workflow tests

### Phase 4: Ergonomics and hardening

- `CommandBus` with routing and middleware hooks
- Idle actor eviction with configurable timeouts
- `tracing` instrumentation throughout
- Benchmarks and performance tuning
- Documentation and examples
