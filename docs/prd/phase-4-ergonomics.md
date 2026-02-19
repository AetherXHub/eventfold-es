# Phase 4: Ergonomics and Hardening

> Ref: [design.md](../design.md) -- section 15, Phase 4
> Depends on: Phase 3

## Goal

Harden the crate for production use: add observability, resource management,
a proper command routing layer, and comprehensive documentation with examples.
No new conceptual features -- this phase polishes what exists.

---

## Prerequisites

- Phase 3 complete (Aggregate, Projections, Process Managers all working).

---

## Deliverables

### Step 1: Tracing instrumentation

**What**: Add structured `tracing` spans and events throughout the crate so users get
visibility into what the system is doing.

**Files**: all `src/*.rs` files

**Instrumentation points**:

| Location | Span / Event | Fields |
|---|---|---|
| `AggregateActor` command receive | `tracing::info_span!("execute")` | `aggregate_type`, `instance_id`, `command` (type name only) |
| `AggregateActor` retry | `tracing::warn!("conflict, retrying")` | `attempt`, `max_retries` |
| `AggregateActor` append success | `tracing::info!("events appended")` | `count`, `end_offset` |
| `ProjectionRunner` catch-up | `tracing::debug_span!("projection_catchup")` | `projection_name` |
| `ProjectionRunner` events applied | `tracing::debug!("events applied")` | `stream`, `count` |
| `ProcessManagerRunner` dispatch | `tracing::info!("dispatching command")` | `target_type`, `target_id` |
| `ProcessManagerRunner` dead-letter | `tracing::error!("command dead-lettered")` | `target_type`, `error` |
| `AggregateStore::get` cache miss | `tracing::debug!("spawning actor")` | `aggregate_type`, `instance_id` |

**Constraints**:

- Never log event payloads or command payloads at `info` level or below (may contain
  PII). Use `debug` or `trace` for payload details.
- All spans include `aggregate_type` and `instance_id` where applicable.

**Tests**:

- Existing tests continue to pass (tracing is purely additive).
- No test for tracing output itself (would require a test subscriber; out of scope).

**Acceptance**:

- `cargo test` passes.
- Clippy clean.
- No `println!` or `eprintln!` anywhere in `src/`.

---

### Step 2: Idle actor eviction

**What**: Aggregate actors that have been idle beyond a configurable timeout are shut
down, releasing file handles and memory.

**File**: `src/actor.rs` (extend), `src/store.rs` (extend)

**Mechanism**:

- Each actor tracks the timestamp of its last received message.
- The actor loop uses `mpsc::Receiver::recv_timeout` (or `tokio::time::timeout` on
  the blocking recv). On timeout, the actor exits its loop and the thread terminates.
- When the actor exits, the `AggregateHandle`'s channel sender detects the closed
  receiver. Subsequent calls to `execute` or `state` return
  `ExecuteError::ActorGone` / `StateError::ActorGone`.
- The `AggregateStore` detects `ActorGone` errors in `get`, evicts the stale handle
  from the cache, and spawns a fresh actor transparently.

**Configuration**:

- `AggregateStoreBuilder::idle_timeout(Duration)` sets the timeout. Default: 5 minutes.
- `Duration::MAX` or a very large value effectively disables eviction.

**Tests** (integration):

- Set a short idle timeout (100ms). Execute a command, sleep beyond the timeout,
  execute another command -- second command succeeds (actor re-spawned) and sees
  the state from the first command.
- Multiple rapid commands do not trigger premature eviction.

**Acceptance**:

- `cargo test` passes.
- Clippy clean.
- New builder method documented.

---

### Step 3: CommandBus

**What**: A typed routing layer that accepts commands and resolves them to the correct
aggregate handle. Provides a single entry point instead of requiring callers to
manually call `store.get::<A>()`.

**File**: `src/command.rs` (extend)

**`CommandBus`**:

```
struct CommandBus { ... }

impl CommandBus {
    fn new(store: AggregateStore) -> Self;

    /// Register a route: commands of type `C` are dispatched to aggregate `A`.
    fn register<A, C>(&mut self)
    where
        A: Aggregate<Command = C>,
        C: Send + 'static;

    /// Dispatch a command to the appropriate aggregate.
    async fn dispatch<C>(
        &self,
        instance_id: &str,
        cmd: C,
        ctx: CommandContext,
    ) -> Result<(), DispatchError>;
}
```

- The bus holds a map of command `TypeId` -> dispatch closure.
- `DispatchError` wraps aggregate-specific errors as a type-erased `Box<dyn Error>`.

**Tests**:

- Register two aggregate types, dispatch commands to each, verify state.
- Dispatching an unregistered command type returns `DispatchError::UnknownCommand`.

**Acceptance**:

- `cargo test` passes.
- Clippy clean.
- Types re-exported from `lib.rs`.

---

### Step 4: Documentation and examples

**What**: Comprehensive crate-level docs and a runnable example.

**Files**: `src/lib.rs` (crate docs), `examples/counter.rs`

**Crate-level docs** (`src/lib.rs`):

- Overview: what the crate does, when to use it, when not to.
- Quick-start code showing aggregate definition, store setup, command execution, and
  projection query.
- Module-level docs for each public module.

**Example** (`examples/counter.rs`):

- A self-contained binary that:
  1. Defines a `Counter` aggregate (commands: Increment, Decrement, Reset).
  2. Defines a `TotalCounter` projection (counts all events across all counters).
  3. Opens a store, creates two counter instances, sends commands, queries projection.
  4. Prints results to stdout.
- Runnable via `cargo run --example counter`.

**Tests**:

- `cargo test --doc` passes (doc examples compile and run).
- `cargo run --example counter` exits 0.

**Acceptance**:

- `cargo test` + `cargo test --doc` passes.
- `cargo clippy -- -D warnings` clean.
- `cargo doc --no-deps` generates docs with no warnings.

---

### Step 5: Cargo.toml metadata + final polish

**What**: Prepare the crate for publishing.

**File**: `Cargo.toml`

**Changes**:

- Add: `description`, `license`, `repository`, `documentation`, `readme`, `keywords`,
  `categories`.
- Review and pin dependency version ranges.
- Ensure `edition = "2024"`.

**Final review**:

- Audit all `pub` items: is the public surface intentional and minimal?
- Check for any remaining `todo!()`, `unimplemented!()`, or `#[allow(...)]`.
- Verify no secrets, credentials, or `.env` files are tracked.

**Acceptance**:

- `cargo publish --dry-run` succeeds.
- `cargo build` + `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
  all clean.

---

## End-of-phase checklist

- [ ] `cargo build` -- no errors, no warnings
- [ ] `cargo test` -- all tests pass (Phase 1 + 2 + 3 + 4)
- [ ] `cargo test --doc` -- doc tests pass
- [ ] `cargo clippy -- -D warnings` -- clean
- [ ] `cargo fmt --check` -- clean
- [ ] `cargo doc --no-deps` -- no warnings
- [ ] `cargo publish --dry-run` -- succeeds
- [ ] All public items have doc comments with examples where appropriate
- [ ] No `unwrap()` in library code
- [ ] No `println!` / `eprintln!` / `dbg!` in library code
- [ ] `tracing` spans on all significant operations
- [ ] Example runs successfully
- [ ] Public API additions:
      `CommandBus`, `DispatchError`,
      `AggregateStoreBuilder::idle_timeout`
