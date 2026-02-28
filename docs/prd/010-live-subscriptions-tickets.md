# Tickets for PRD 010: Live Subscriptions

**Source PRD:** docs/prd/010-live-subscriptions.md
**Created:** 2026-02-27
**Total Tickets:** 5
**Estimated Total Complexity:** 13 (S=1, M=2, L=3)

---

### Ticket 1: LiveConfig struct and builder integration

**Description:**
Add the `LiveConfig` struct with `Default` impl and wire it into
`AggregateStoreBuilder` and `AggregateStore`. This is pure plumbing --
no live loop logic yet. The config is stored on `AggregateStore` for
later use by `start_live()`.

**Scope:**
- Create: `src/live.rs`
  - Add `LiveConfig` struct with `checkpoint_interval`, `reconnect_base_delay`,
    `reconnect_max_delay` fields, all `Duration`. Derive `Debug, Clone`.
  - Implement `Default` for `LiveConfig` with values: 5s, 1s, 30s.
- Modify: `src/store.rs`
  - Add `live_config: LiveConfig` field to `AggregateStore`.
  - Add `live_config: LiveConfig` field to `AggregateStoreBuilder` (default via
    `LiveConfig::default()`).
  - Add `pub fn live_config(mut self, config: LiveConfig) -> Self` to builder.
  - Initialize `live_config` in `AggregateStoreBuilder::open()`.
  - Update `mock_store()` in tests to include the new field.
- Modify: `src/lib.rs`
  - Add `mod live;` declaration.
  - Re-export `LiveConfig` from `crate::live`.

**Acceptance Criteria:**
- [ ] `LiveConfig::default()` returns `checkpoint_interval = 5s`,
      `reconnect_base_delay = 1s`, `reconnect_max_delay = 30s` (AC 10).
- [ ] `AggregateStoreBuilder::new().live_config(custom).open()` stores the
      custom config (verified by unit test reading the field) (AC 11).
- [ ] `LiveConfig` is publicly importable from `eventfold_es::LiveConfig` (AC 14).
- [ ] `LiveConfig` has doc comments on struct and all fields (AC 14).
- [ ] `cargo test` passes with no new failures.
- [ ] `cargo clippy -- -D warnings` passes.

**Dependencies:** None
**Complexity:** S
**Maps to PRD AC:** AC 10, AC 11, AC 14

---

### Ticket 2: Refactor projection/PM runner internals for live mode access

**Description:**
Add type-erased trait methods that the live loop needs to interact with
projections and process managers. The live loop will process a single
`RecordedEvent` at a time (not a full stream), so we need per-event
`apply_event` and `react_event` methods on the type-erased traits.
Also add `position()` and `state_clone()` accessors.

**Scope:**
- Modify: `src/projection.rs`
  - Add a type-erased trait `ProjectionCatchUp: Send + Sync` with methods:
    - `fn catch_up(&mut self) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>>`
    - `fn apply_event(&mut self, recorded: &proto::RecordedEvent)` -- decodes and
      applies a single event, advancing the checkpoint position.
    - `fn position(&self) -> u64` -- returns `checkpoint.last_global_position`.
    - `fn save(&self) -> io::Result<()>` -- saves checkpoint to disk.
    - `fn state_any(&self) -> Box<dyn Any + Send>` -- clones state into a `Box<dyn Any>`.
  - Implement `ProjectionCatchUp` for `ProjectionRunner<P>`.
  - Factor event decode+apply logic from `process_stream` into a shared helper
    used by both `process_stream` and `apply_event`.
- Modify: `src/process_manager.rs`
  - Add to existing `ProcessManagerCatchUp` trait:
    - `fn react_event(&mut self, recorded: &proto::RecordedEvent) -> Vec<CommandEnvelope>`
      -- decodes and reacts to a single event, advancing the checkpoint position.
    - `fn position(&self) -> u64` -- returns checkpoint position.
  - Implement these for `ProcessManagerRunner<PM>`.
- Modify: `src/store.rs`
  - Change the `ProjectionMap` type alias to store
    `tokio::sync::Mutex<Box<dyn ProjectionCatchUp>>` instead of
    `tokio::sync::Mutex<ProjectionRunner<P>>` erased via `dyn Any`.
  - Update the projection factory in `AggregateStoreBuilder::projection::<P>()`
    to produce the new type.
  - Update `AggregateStore::projection::<P>()` to use `state_any()` and
    downcast to `P`, replacing the current `dyn Any` downcast of the runner.

**Acceptance Criteria:**
- [ ] All existing projection and process manager tests pass unchanged.
- [ ] All existing store tests pass unchanged.
- [ ] `store.projection::<P>()` returns the same results as before the refactor.
- [ ] `store.run_process_managers()` returns the same results as before.
- [ ] `cargo test` passes with no failures.
- [ ] `cargo clippy -- -D warnings` passes.

**Dependencies:** Ticket 1
**Complexity:** L
**Maps to PRD AC:** AC 12 (pull-based unchanged)

---

### Ticket 3: LiveHandle and live subscription loop

**Description:**
Implement the core live subscription loop in `src/live.rs`. This is the
main feature: a background tokio task that holds a `SubscribeAll` stream
open, fans out events to all projections and process managers, dispatches
PM command envelopes, checkpoints periodically, reconnects on error with
exponential backoff, and shuts down gracefully.

**Scope:**
- Modify: `src/live.rs`
  - Add `LiveHandle` struct with:
    - `shutdown_tx: tokio::sync::watch::Sender<bool>` (signals the loop to stop)
    - `caught_up: Arc<std::sync::atomic::AtomicBool>` (set when CaughtUp received)
    - `task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<io::Result<()>>>>>`
    - `pub async fn shutdown(&self) -> io::Result<()>` -- sends shutdown signal,
      awaits the task, returns its result.
    - `pub fn is_caught_up(&self) -> bool` -- reads the atomic.
  - Add `pub(crate) async fn run_live_loop(...)` function that:
    1. Computes the minimum global position across all projections and PMs.
    2. Calls `client.subscribe_all_from(min_position)`.
    3. Reads events from the stream in a `tokio::select!` loop:
       - On event: decode, fan out to all projections (`apply_event`) and PMs
         (`react_event`), dispatch PM envelopes, advance cursor.
       - On `CaughtUp`: set `caught_up` flag, reset backoff.
       - On checkpoint timer tick: save all projection and PM checkpoints.
       - On shutdown signal: save all checkpoints, break.
       - On stream error/EOF: save checkpoints, backoff, reconnect.
    4. PM envelope dispatch uses the store's `dispatchers` map with
       dead-lettering on failure (same pattern as `run_process_managers`).
  - Derive `Clone` for `LiveHandle`.
- Modify: `src/lib.rs`
  - Re-export `LiveHandle` from `crate::live`.

**Acceptance Criteria:**
- [ ] `LiveHandle` is publicly importable from `eventfold_es::LiveHandle` (AC 14).
- [ ] `LiveHandle::shutdown()` saves final checkpoints and the task exits (AC 8).
- [ ] `LiveHandle::is_caught_up()` returns `true` after `CaughtUp` received (AC 3).
- [ ] The loop processes events from the stream and fans out to projections
      (verified via unit test with mock stream) (AC 4, AC 5).
- [ ] PM envelopes are dispatched during the loop (AC 6).
- [ ] Dead-lettered envelopes go to the same per-PM log files (AC 7).
- [ ] Exponential backoff on reconnect respects `LiveConfig` values (AC 9).
- [ ] `LiveHandle` and all public methods have doc comments (AC 14).
- [ ] `cargo test` passes.
- [ ] `cargo clippy -- -D warnings` passes.

**Dependencies:** Ticket 1, Ticket 2
**Complexity:** L
**Maps to PRD AC:** AC 3, AC 4, AC 5, AC 6, AC 7, AC 8, AC 9, AC 14

---

### Ticket 4: AggregateStore start_live() and live_projection()

**Description:**
Wire the live loop into `AggregateStore` with two new public methods:
`start_live()` and `live_projection::<P>()`.

**Scope:**
- Modify: `src/store.rs`
  - Add `live_handle: Arc<tokio::sync::Mutex<Option<LiveHandle>>>` field to
    `AggregateStore`. Initialize as `None`.
  - Add `pub async fn start_live(&self) -> io::Result<LiveHandle>`:
    - Takes the mutex, checks `is_some()` -- return
      `Err(io::Error::new(AlreadyExists, ...))` if already started (AC 2).
    - Calls `run_live_loop(...)` passing clones of projections, process_managers,
      dispatchers, client, and live_config.
    - Spawns the future as a tokio task.
    - Stores the `LiveHandle` in the mutex and returns a clone.
  - Add `pub async fn live_projection<P: Projection>(&self) -> io::Result<P>`:
    - If live mode is active, read projection state via `state_any()` +
      downcast without triggering catch-up.
    - If live mode is not active, fall back to `self.projection::<P>()`.
  - Update `mock_store()` in tests to include the new field.

**Acceptance Criteria:**
- [ ] `store.start_live().await` returns `Ok(handle)` (AC 1).
- [ ] Second `store.start_live().await` returns `Err` with
      `ErrorKind::AlreadyExists` (AC 2).
- [ ] `store.live_projection::<P>()` returns state without catch-up (AC 4, AC 5).
- [ ] `store.live_projection::<P>()` falls back to pull-based catch-up when live
      mode is not active (AC 12).
- [ ] `store.projection::<P>()` still works whether or not live mode is active
      (AC 12).
- [ ] `start_live` and `live_projection` have doc comments (AC 14).
- [ ] `cargo test` passes.
- [ ] `cargo clippy -- -D warnings` passes.

**Dependencies:** Ticket 3
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 2, AC 4, AC 5, AC 12, AC 14

---

### Ticket 5: Verification and integration check

**Description:**
Run the full PRD acceptance criteria checklist end-to-end. Verify that all
tickets integrate correctly and that no regressions exist in the existing
test suite. Confirm linting, formatting, and documentation.

**Acceptance Criteria:**
- [ ] All PRD acceptance criteria (ACs 1-14) are verified by tests.
- [ ] `cargo test` passes with zero failures.
- [ ] `cargo clippy -- -D warnings` passes with zero warnings.
- [ ] `cargo fmt --check` passes with no formatting violations.
- [ ] `cargo doc --no-deps` succeeds; `LiveConfig`, `LiveHandle`,
      `AggregateStore::start_live`, and `AggregateStore::live_projection`
      appear in the generated HTML with non-empty doc comments.
- [ ] No regressions in existing actor, store, projection, or process-manager
      test suites.

**Dependencies:** Ticket 1, Ticket 2, Ticket 3, Ticket 4
**Complexity:** S
**Maps to PRD AC:** AC 13

---

## Dependency Graph

```
Ticket 1 (LiveConfig + builder)
    |
    v
Ticket 2 (projection/PM refactor)
    |
    v
Ticket 3 (live loop + LiveHandle)
    |
    v
Ticket 4 (store.start_live + live_projection)
    |
    v
Ticket 5 (verification)
```

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1 | `start_live()` returns `Ok(handle)` | Ticket 4 | Covered |
| 2 | Second `start_live()` returns `Err` | Ticket 4 | Covered |
| 3 | `is_caught_up()` returns `true` after CaughtUp | Ticket 3 | Covered |
| 4 | `live_projection` reflects events after CaughtUp | Ticket 3, Ticket 4 | Covered |
| 5 | `live_projection` reflects new events without manual catch-up | Ticket 3, Ticket 4 | Covered |
| 6 | PM envelopes dispatched automatically in live mode | Ticket 3 | Covered |
| 7 | Dead-lettered envelopes written to same per-PM log | Ticket 3 | Covered |
| 8 | `shutdown()` saves checkpoints and task exits | Ticket 3 | Covered |
| 9 | Reconnect with exponential backoff | Ticket 3 | Covered |
| 10 | `LiveConfig::default()` values | Ticket 1 | Covered |
| 11 | `live_config()` builder method stores config | Ticket 1 | Covered |
| 12 | Pull-based `projection()` unchanged | Ticket 2, Ticket 4 | Covered |
| 13 | cargo test + clippy + fmt all pass | Ticket 5 | Covered |
| 14 | All new public types/methods have doc comments | Ticket 1, Ticket 3, Ticket 4 | Covered |
