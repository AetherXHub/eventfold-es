# Phase 2: Projections

> Ref: [design.md](../design.md) -- sections 5.2, 8
> Depends on: Phase 1

## Goal

Add cross-stream read models. After this phase, users can define projections that
fold events from multiple aggregate streams into queryable state:

```rust
let store = AggregateStore::builder(tmp.path())
    .projection::<OrderSummary>()
    .open()
    .await?;

let handle = store.get::<Order>("ord-1").await?;
handle.execute(OrderCommand::Place { total: 42.0 }, ctx).await?;

let summary: &OrderSummary = store.projection::<OrderSummary>().await?;
assert_eq!(summary.total_orders, 1);
```

Projections are eventually consistent, persist their state and per-stream cursors to
disk, and automatically rebuild from scratch if their checkpoint is invalid.

---

## Prerequisites

- Phase 1 complete (Aggregate trait, AggregateStore, actor system).
- Design doc sections 5.2 and 8 reviewed.

---

## Deliverables

### Step 1: Projection trait

**What**: Define the `Projection` trait and supporting types.

**File**: `src/projection.rs`

**`Projection` trait**:

```
trait Projection: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;
    fn subscriptions(&self) -> &'static [&'static str];
    fn apply(&mut self, aggregate_type: &str, stream_id: &str, event: &eventfold::Event);
}
```

**`ProjectionCursor`** (internal):

- Tracks per-stream progress: `HashMap<(String, String), CursorPosition>` where
  `CursorPosition` is `{ offset: u64, hash: String }`.
- Serializable so it can be persisted alongside projection state.

**`ProjectionCheckpoint<P>`** (persisted to disk):

```
struct ProjectionCheckpoint<P> {
    state: P,
    cursors: HashMap<(String, String), CursorPosition>,
}
```

**Tests**:

- Define a test projection (`OrderCounter`) in `#[cfg(test)]`.
- Unit test `apply` logic in isolation (no I/O).
- Checkpoint serialization round-trip.

**Acceptance**:

- `cargo build` + `cargo test` + clippy clean.
- Trait re-exported from `lib.rs`.

---

### Step 2: Projection persistence

**What**: Load and save projection checkpoints to disk.

**File**: `src/projection.rs` (extend)

**Functions**:

```
fn save_checkpoint<P: Projection>(dir: &Path, checkpoint: &ProjectionCheckpoint<P>) -> io::Result<()>
fn load_checkpoint<P: Projection>(dir: &Path) -> io::Result<Option<ProjectionCheckpoint<P>>>
fn delete_checkpoint<P: Projection>(dir: &Path) -> io::Result<()>
```

- Checkpoints are written atomically (write to `.tmp`, rename).
- Stored at `<base_dir>/projections/<NAME>/checkpoint.json`.

**Tests** (integration, `tempfile`):

- Save then load round-trips correctly.
- Load from empty directory returns `None`.
- Delete removes the file.
- Corrupt file triggers `None` on load (not a hard error -- projection will rebuild).

**Acceptance**:

- `cargo test` passes.
- Clippy clean.

---

### Step 3: Catch-up loop

**What**: The core projection engine that reads new events from subscribed streams and
applies them.

**File**: `src/projection.rs` (extend)

**`ProjectionRunner<P: Projection>`** (internal):

- Holds:
  - The projection state (`ProjectionCheckpoint<P>`).
  - A `StreamLayout` reference (to discover and resolve stream paths).
  - `EventReader` instances (created on demand per stream).
- Methods:
  - `catch_up(&mut self) -> Result<()>`:
    - Discovers all streams for each subscribed aggregate type.
    - For each stream, reads events from the cursor's last offset.
    - Calls `P::apply` for each event.
    - Updates cursors.
    - Saves checkpoint.
  - `state(&self) -> &P`: returns current projection state.
  - `rebuild(&mut self) -> Result<()>`: deletes checkpoint, replays everything.

**Ordering**: within each stream, events are processed in order. Across streams,
the interleaving order is deterministic within a single `catch_up` call (iterate
streams in sorted order by `(aggregate_type, instance_id)`) but not guaranteed
across calls.

**Tests** (integration):

- Create a store, append events to two aggregate instances via handles, run catch-up,
  verify projection state reflects all events.
- Incremental catch-up: append more events, catch up again, verify state is correct
  without re-processing old events (assert cursor offsets advanced).
- New stream discovery: create a new aggregate instance after initial catch-up, catch
  up again, verify the new stream's events are picked up.
- Rebuild: corrupt the checkpoint, rebuild, verify state matches a fresh replay.

**Acceptance**:

- `cargo test` passes.
- Clippy clean.

---

### Step 4: AggregateStore integration

**What**: Wire projections into the `AggregateStore` so they catch up after commands
and are queryable.

**Files**: `src/store.rs` (extend), `src/lib.rs` (re-exports)

**Changes to `AggregateStore`**:

- Add a builder pattern: `AggregateStore::builder(base_dir)` returns an
  `AggregateStoreBuilder`.
  - `.projection::<P>()` registers a projection type.
  - `.open()` finishes construction (replaces the current `AggregateStore::open`).
  - `AggregateStore::open(base_dir)` remains as a convenience (no projections).
- Add method: `projection<P: Projection>(&self) -> Result<P>` returns a clone of the
  current projection state.
- After each successful `execute` on an `AggregateHandle`, the store triggers a
  catch-up for all registered projections. This is done asynchronously (the execute
  call does not block on projection catch-up).

**Event notification mechanism**:

- The actor sends a notification (via a `tokio::sync::broadcast` or
  `tokio::sync::watch`) after each successful append.
- A background task per projection listens for notifications and runs catch-up.
- Alternatively, projections catch up lazily when `store.projection::<P>()` is called.
  (The simpler approach; the background task can be deferred to Phase 4.)

**Decision**: use the lazy approach for Phase 2. `store.projection::<P>()` triggers
a catch-up before returning the state. This keeps the implementation simple and avoids
background task lifecycle management. It does mean projection reads pay the cost of
catch-up, but for embedded use cases this is acceptable. Phase 4 can add background
catch-up as an optimization.

**Tests** (integration):

- Full round-trip: build store with projection, execute commands, query projection.
- Projection sees events from multiple aggregate types.
- Projection state persists across store restarts.
- Projection rebuilds from scratch if checkpoint is missing.

**Acceptance**:

- `cargo test` passes, including all Phase 1 tests (no regressions).
- `cargo clippy -- -D warnings` clean.
- `cargo fmt --check` clean.
- New public items have doc comments.
- Re-exports updated: `Projection`, `AggregateStoreBuilder`.

---

## End-of-phase checklist

- [ ] `cargo build` -- no errors, no warnings
- [ ] `cargo test` -- all tests pass (Phase 1 + Phase 2)
- [ ] `cargo clippy -- -D warnings` -- clean
- [ ] `cargo fmt --check` -- clean
- [ ] All public items have doc comments
- [ ] No `unwrap()` in library code
- [ ] Public API additions:
      `Projection`, `AggregateStoreBuilder`, `AggregateStore::builder`,
      `AggregateStore::projection`
