# Phase 3: Process Managers

> Ref: [design.md](../design.md) -- sections 5.3, 8
> Depends on: Phase 2

## Goal

Add cross-aggregate workflow coordination. After this phase, users can define process
managers that react to events in one aggregate by dispatching commands to another:

```rust
let store = AggregateStore::builder(tmp.path())
    .process_manager::<ReservationSaga>()
    .open()
    .await?;

// Place an order -- the saga reacts by reserving inventory.
let order = store.get::<Order>("ord-1").await?;
order.execute(OrderCommand::Place { sku: "W-1", qty: 2 }, ctx).await?;

// The saga dispatched a command to the Inventory aggregate.
let inv = store.get::<Inventory>("inv-W-1").await?;
let inv_state = inv.state().await?;
assert_eq!(inv_state.reserved, 2);
```

Process managers maintain persisted state and cursors (like projections) but produce
side effects (commands) rather than read models. Failed command dispatches are captured
in a dead-letter log for manual inspection.

---

## Prerequisites

- Phase 2 complete (projections, AggregateStoreBuilder, catch-up loop).
- The catch-up loop infrastructure from Phase 2 is reusable for process managers.

---

## Deliverables

### Step 1: CommandEnvelope + ProcessManager trait

**What**: Define the types that process managers produce and the trait itself.

**File**: `src/command.rs` (extend), `src/process_manager.rs` (new)

**`CommandEnvelope`**:

```
struct CommandEnvelope {
    pub aggregate_type: String,
    pub instance_id: String,
    pub command: Value,
    pub context: CommandContext,
}
```

- Carries a type-erased command payload (`serde_json::Value`) because the process
  manager may target aggregate types different from the one it subscribes to.
- The dispatch layer deserializes `command` into the concrete `A::Command` using the
  `aggregate_type` to look up the target aggregate.

**`ProcessManager` trait**:

```
trait ProcessManager: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;
    fn subscriptions(&self) -> &'static [&'static str];
    fn react(
        &mut self,
        aggregate_type: &str,
        stream_id: &str,
        event: &eventfold::Event,
    ) -> Vec<CommandEnvelope>;
}
```

**Tests**:

- Define a test process manager (`EchoSaga`) in `#[cfg(test)]` that reacts to events
  by producing a command envelope.
- Unit test: given an event, `react` returns the expected envelopes.
- `CommandEnvelope` serialization round-trip.

**Acceptance**:

- `cargo build` + `cargo test` + clippy clean.
- Types re-exported from `lib.rs`.

---

### Step 2: Process manager persistence + catch-up

**What**: Reuse the cursor/checkpoint infrastructure from projections for process
managers. Add the catch-up loop that also dispatches commands.

**File**: `src/process_manager.rs` (extend)

**`ProcessManagerRunner<PM: ProcessManager>`** (internal):

- Structurally similar to `ProjectionRunner<P>`: holds checkpoint with state + cursors,
  discovers streams, reads from cursors, calls `PM::react`.
- After each `react` call, collected `CommandEnvelope`s are returned to the caller
  (the store) for dispatch.
- Checkpoint is saved after each successful catch-up pass (after all envelopes for
  that pass have been dispatched).

**Checkpoint format**: same as projections but stored under
`<base_dir>/process_managers/<NAME>/checkpoint.json`.

**Tests** (integration):

- Catch-up produces the expected command envelopes.
- Cursors advance; re-running catch-up does not re-emit old envelopes.
- Checkpoint persists and restores.
- Rebuild replays all events.

**Acceptance**:

- `cargo test` passes.
- Clippy clean.

---

### Step 3: Command dispatch + dead-letter log

**What**: The store dispatches command envelopes produced by process managers and
records failures.

**File**: `src/store.rs` (extend), `src/process_manager.rs` (extend)

**Dispatch flow**:

1. `store.projection::<P>()` was the lazy catch-up trigger for projections. Process
   managers use a similar approach: `store.run_process_managers()` catches up all
   registered process managers and dispatches their envelopes.
2. For each `CommandEnvelope`:
   - Look up the target aggregate type in a type registry.
   - Deserialize `envelope.command` into the concrete `A::Command`.
   - Call `store.get::<A>(&envelope.instance_id).await?.execute(cmd, ctx).await`.
3. If dispatch fails (deserialization error, command rejection, I/O), the envelope is
   written to a dead-letter log.

**Type registry**:

- The `AggregateStoreBuilder` gains a method: `.aggregate_type::<A>()` which registers
  `A::AGGREGATE_TYPE` string with a dispatch function
  `fn(store, envelope) -> Result<()>`.
- This is necessary because process managers produce type-erased envelopes; the store
  needs to know how to deserialize and route them.

**Dead-letter log**:

- Stored at `<base_dir>/process_managers/<NAME>/dead_letters.jsonl`.
- Each entry: `{ "envelope": ..., "error": "...", "ts": ... }`.
- Append-only, not an `eventfold` log (just plain JSONL for simplicity).

**Tests** (integration):

- End-to-end: event in aggregate A triggers process manager, which dispatches command
  to aggregate B, whose state updates accordingly.
- Failed dispatch (command rejected) writes to dead-letter log.
- Dead-letter log is human-readable JSONL.
- Process manager does not re-dispatch on subsequent catch-up (cursors advanced past
  the triggering event).

**Acceptance**:

- `cargo test` passes.
- Clippy clean.

---

### Step 4: AggregateStore integration

**What**: Wire process managers into the builder and provide a query API.

**Files**: `src/store.rs` (extend), `src/lib.rs` (re-exports)

**Changes to `AggregateStoreBuilder`**:

- `.process_manager::<PM>()` registers a process manager.
- `.aggregate_type::<A>()` registers a dispatchable aggregate type (required for
  process manager command routing).

**Changes to `AggregateStore`**:

- `run_process_managers(&self) -> Result<ProcessManagerReport>`:
  catches up all process managers, dispatches envelopes, returns a report of
  successes and dead-letters.
- The report is a simple struct:
  `ProcessManagerReport { dispatched: usize, dead_lettered: usize }`.

**Tests** (integration):

- Full saga round-trip: OrderPlaced -> ReservationSaga -> ReserveInventory.
- Multiple process managers coexist.
- `run_process_managers` is idempotent when no new events exist.
- Store restart recovers process manager state; no duplicate dispatches.

**Acceptance**:

- `cargo test` passes (Phase 1 + 2 + 3, no regressions).
- `cargo clippy -- -D warnings` clean.
- `cargo fmt --check` clean.
- New public items have doc comments.
- Re-exports updated: `ProcessManager`, `CommandEnvelope`,
  `ProcessManagerReport`.

---

## End-of-phase checklist

- [ ] `cargo build` -- no errors, no warnings
- [ ] `cargo test` -- all tests pass (Phase 1 + 2 + 3)
- [ ] `cargo clippy -- -D warnings` -- clean
- [ ] `cargo fmt --check` -- clean
- [ ] All public items have doc comments
- [ ] No `unwrap()` in library code
- [ ] Public API additions:
      `ProcessManager`, `CommandEnvelope`, `ProcessManagerReport`,
      `AggregateStoreBuilder::process_manager`,
      `AggregateStoreBuilder::aggregate_type`,
      `AggregateStore::run_process_managers`
