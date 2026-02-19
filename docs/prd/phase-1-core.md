# Phase 1: Core Aggregate Loop

> Ref: [design.md](../design.md) -- sections 5.1, 6, 7, 9, 10

## Goal

Deliver a working library crate where users can define aggregates, send commands,
and read state through an async API. At the end of this phase, the following works
end-to-end:

```rust
let store = AggregateStore::open(tmp.path()).await?;
let handle = store.get::<Counter>("counter-1").await?;

handle.execute(CounterCommand::Increment, CommandContext::default()).await?;
handle.execute(CounterCommand::Increment, CommandContext::default()).await?;

let state = handle.state().await?;
assert_eq!(state.value, 2);
```

Events are durably persisted in the `eventfold` directory layout described in the
design doc. Restarting the process and re-opening the same store recovers all state
from disk.

---

## Prerequisites

- `eventfold = "0.2.0"` on crates.io.
- Familiarity with the design doc, specifically the async actor model and storage layout.

---

## Deliverables

Each step leaves the crate in a green state: `cargo build`, `cargo test`,
`cargo clippy -- -D warnings`, and `cargo fmt --check` all pass with no warnings
or errors.

### Step 1: Crate scaffold

**What**: Convert the binary crate to a library crate. Add dependencies. Create the
module file tree with empty stubs.

**Changes**:

- Delete `src/main.rs`.
- Create `src/lib.rs` with `mod` declarations for: `aggregate`, `actor`, `command`,
  `error`, `storage`. Each module file is created with a placeholder doc comment
  only (no types yet).
- Update `Cargo.toml`:
  - Remove `[[bin]]` if present. Ensure `[lib]` target.
  - Add dependencies:
    - `tokio = { version = "1", features = ["rt-multi-thread", "sync", "macros"] }`
    - `thiserror = "2"`
    - `serde = { version = "1", features = ["derive"] }`
    - `serde_json = "1"`
    - `tracing = "0.1"`
  - Add dev-dependency: `tempfile = "3"`, `tokio` with `test-util` feature.

**Acceptance**:

- `cargo build` succeeds.
- `cargo clippy -- -D warnings` clean.
- `cargo fmt --check` clean.
- No tests yet; `cargo test` runs and reports 0 tests.

---

### Step 2: Error types

**What**: Define the crate's error types so all subsequent code can use them.

**File**: `src/error.rs`

**Types**:

```
ExecuteError<A: Aggregate>
    Domain(A::Error)        -- command rejected by aggregate logic
    Conflict                -- optimistic concurrency retries exhausted
    Io(io::Error)           -- disk I/O failure
    ActorGone               -- actor thread exited unexpectedly

StateError
    Io(io::Error)
    ActorGone
```

- Both enums derive `Debug` and implement `std::fmt::Display` and `std::error::Error`
  via `thiserror`.
- `ExecuteError` is generic over `A: Aggregate` so it can carry `A::Error` in the
  `Domain` variant. Since `Aggregate` is not yet defined, this step forward-declares
  the trait bound.

**Acceptance**:

- `cargo build` succeeds.
- Error types are re-exported from `lib.rs`.
- Clippy clean.

---

### Step 3: Aggregate trait + CommandContext

**What**: Define the `Aggregate` trait and `CommandContext` struct.

**Files**: `src/aggregate.rs`, `src/command.rs`

**`Aggregate` trait** (as specified in design doc section 5.1):

- Associated const `AGGREGATE_TYPE: &'static str`.
- Associated types: `Command`, `DomainEvent`, `Error`.
- Methods: `handle(&self, cmd) -> Result<Vec<DomainEvent>, Error>`,
  `apply(self, &DomainEvent) -> Self`.
- Supertraits: `Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static`.

**`CommandContext`** (as specified in design doc section 5.1.1):

- Fields: `actor: Option<String>`, `correlation_id: Option<String>`,
  `metadata: Option<Value>`.
- Derive `Debug, Clone, Default`.
- Builder methods: `with_actor`, `with_correlation_id`, `with_metadata`.

**Test aggregate** (in `src/aggregate.rs` `#[cfg(test)]` module):

- `Counter` struct with `value: u64`.
- `CounterCommand` enum: `Increment`, `Decrement`, `Add(u64)`.
- `CounterEvent` enum: `Incremented`, `Decremented`, `Added { amount: u64 }`.
- `CounterError` enum: `AlreadyZero`.
- Impl `Aggregate for Counter`.
- Unit tests:
  - `handle` produces correct events for each command variant.
  - `handle` returns error when decrementing at zero.
  - `apply` folds each event variant correctly.
  - Round-trip: `handle` then `apply` each produced event yields expected state.

**Acceptance**:

- `cargo test` passes with all unit tests green.
- Trait and types are re-exported from `lib.rs`.
- Clippy clean.

---

### Step 4: Storage layer

**What**: Functions to manage the on-disk directory layout and stream registry.

**File**: `src/storage.rs`

**Functions / types**:

```
StreamLayout
    new(base_dir) -> Self
    stream_dir(&self, aggregate_type, instance_id) -> PathBuf
    views_dir(&self, aggregate_type, instance_id) -> PathBuf
    projections_dir(&self) -> PathBuf
    meta_dir(&self) -> PathBuf
    ensure_stream(&self, aggregate_type, instance_id) -> io::Result<PathBuf>
    list_streams(&self, aggregate_type) -> io::Result<Vec<String>>
```

- `ensure_stream` creates the directory tree if it does not exist and appends a
  registration entry to `meta/streams.jsonl`.
- `list_streams` reads the filesystem to discover instance IDs for a given aggregate
  type (reads directory entries under `streams/<aggregate_type>/`).
- The `meta/streams.jsonl` entries are JSON: `{"type": "...", "id": "...", "ts": ...}`.

**Tests** (unit + integration using `tempfile::TempDir`):

- `ensure_stream` creates expected directory structure.
- `ensure_stream` is idempotent (calling twice does not error or duplicate registry
  entries).
- `list_streams` returns the correct IDs after creating multiple streams.
- `list_streams` returns empty vec for unknown aggregate types.
- Path helpers return correct paths.

**Acceptance**:

- `cargo test` passes.
- Types re-exported from `lib.rs`.
- Clippy clean.

---

### Step 5: ReduceFn adapter

**What**: Bridge between `Aggregate::apply` and `eventfold::ReduceFn<A>`. Converts
`DomainEvent` to/from `eventfold::Event`.

**File**: `src/aggregate.rs` (added to the existing module)

**Functions**:

```
/// Build a `ReduceFn<A>` that deserializes `eventfold::Event` into `A::DomainEvent`
/// and calls `A::apply`.
pub fn reducer<A: Aggregate>() -> ReduceFn<A>

/// Convert a `DomainEvent` + `CommandContext` into an `eventfold::Event`.
pub fn to_eventfold_event<A: Aggregate>(
    domain_event: &A::DomainEvent,
    ctx: &CommandContext,
) -> serde_json::Result<eventfold::Event>
```

- `reducer` returns a `fn(A, &eventfold::Event) -> A` that:
  - Attempts to deserialize `event.data` as `A::DomainEvent`.
  - On success, calls `A::apply(state, &domain_event)`.
  - On failure (unknown event type), returns state unchanged (forward compatibility).
- `to_eventfold_event` serializes the `DomainEvent` to `serde_json::Value`, extracts
  the serde tag as `event_type`, and populates `actor`/`meta` from `CommandContext`.

**Serialization convention**: `DomainEvent` is serialized with
`#[serde(tag = "type", content = "data")]` (adjacently tagged). The `type` field
becomes the `event_type` on `eventfold::Event`. The `data` field (or unit for
fieldless variants) becomes `eventfold::Event::data`.

**Tests**:

- Round-trip: `to_eventfold_event` then deserialized by the `reducer` yields the
  original domain event applied to state.
- Unknown event types are silently skipped (state unchanged).
- `CommandContext` fields propagate to `eventfold::Event` fields.
- Fieldless enum variants round-trip correctly.

**Acceptance**:

- `cargo test` passes.
- Clippy clean.

---

### Step 6: Actor system

**What**: The `AggregateActor` (blocking thread) and `AggregateHandle` (async handle).

**File**: `src/actor.rs`

**`AggregateActor<A: Aggregate>`**:

- Spawned on a dedicated `std::thread` (via `tokio::task::spawn_blocking` or
  `std::thread::spawn` with a `tokio::sync::mpsc::Receiver`).
- Owns:
  - `EventWriter` (from `eventfold`).
  - `View<A>` (aggregate state view, using the `reducer::<A>()` adapter).
  - `EventReader` (for refreshing the view).
  - `mpsc::Receiver<ActorMessage<A>>`.
- `ActorMessage<A>` is an internal enum:
  - `Execute { cmd: A::Command, ctx: CommandContext, reply: oneshot::Sender<...> }`
  - `GetState { reply: oneshot::Sender<...> }`
  - `Shutdown`
- The actor loop (as described in design doc section 6):
  1. Receive message.
  2. Refresh view.
  3. Handle command / read state.
  4. For commands: convert events, `append_if`, retry on conflict.
  5. Reply.
- Configurable max retries (default: 3).

**`AggregateHandle<A: Aggregate>`**:

- Holds `mpsc::Sender<ActorMessage<A>>` and an `EventReader` clone.
- `Clone + Send + Sync`.
- Methods:
  - `execute(&self, cmd, ctx) -> Result<Vec<A::DomainEvent>, ExecuteError<A>>`
  - `state(&self) -> Result<A, StateError>`

**Spawning**:

- A free function or associated method:
  `spawn_actor<A: Aggregate>(stream_dir: &Path) -> io::Result<AggregateHandle<A>>`
- Opens `EventWriter`, creates `EventReader`, creates `View<A>`, spawns thread.

**Tests** (integration, using `tempfile::TempDir`):

- Spawn actor for a `Counter` aggregate, execute `Increment` 3 times, read state,
  assert `value == 3`.
- Execute `Decrement` on zero returns `ExecuteError::Domain(AlreadyZero)`.
- State persists: drop the handle and actor, re-spawn from the same directory, read
  state, assert value is preserved.
- Multiple sequential commands produce the correct final state.

**Acceptance**:

- `cargo test` passes.
- Types re-exported from `lib.rs`.
- Clippy clean.

---

### Step 7: AggregateStore

**What**: The top-level entry point that ties storage, actors, and handles together.

**File**: `src/store.rs`

**`AggregateStore`**:

- Constructed via `AggregateStore::open(base_dir)`.
- Internally holds:
  - `StreamLayout` (from storage module).
  - A handle cache: `HashMap<(String, String), Box<dyn Any + Send + Sync>>` keyed by
    `(aggregate_type, instance_id)`, values are `AggregateHandle<A>` (type-erased).
  - The cache is behind an `Arc<tokio::sync::RwLock<...>>` so the store itself is
    `Clone + Send + Sync`.
- Methods:
  - `open(base_dir) -> Result<Self>`: creates `StreamLayout`, ensures `meta/` dir.
  - `get<A: Aggregate>(&self, id: &str) -> Result<AggregateHandle<A>>`:
    checks cache, spawns actor if missing, returns handle.
  - `list<A: Aggregate>(&self) -> Result<Vec<String>>`:
    delegates to `StreamLayout::list_streams`.

**Tests** (integration, using `tempfile::TempDir` + `#[tokio::test]`):

- Open store, get handle, execute commands, read state -- full round-trip.
- `list` returns empty initially, returns `["counter-1"]` after first command to
  `counter-1`.
- Getting the same aggregate ID twice returns handles that see the same state
  (commands through one handle are visible via state on the other).
- Opening a store on an existing data directory recovers prior state.
- Two different aggregate types coexist in the same store.

**Acceptance**:

- `cargo test` passes, including all integration tests.
- `AggregateStore` is re-exported from `lib.rs`.
- `cargo clippy -- -D warnings` clean.
- `cargo fmt --check` clean.
- Full public API has doc comments.

---

## End-of-phase checklist

- [ ] `cargo build` -- no errors, no warnings
- [ ] `cargo test` -- all tests pass
- [ ] `cargo clippy -- -D warnings` -- clean
- [ ] `cargo fmt --check` -- clean
- [ ] All public items have doc comments
- [ ] No `unwrap()` in library code
- [ ] No debug `println!` or `dbg!` statements
- [ ] Re-exports in `lib.rs` form a coherent public API:
      `Aggregate`, `CommandContext`, `AggregateStore`, `AggregateHandle`,
      `ExecuteError`, `StateError`
