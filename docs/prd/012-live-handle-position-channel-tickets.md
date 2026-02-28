# Tickets for PRD 012: LiveHandle Position Notification Channel

**Source PRD:** docs/prd/012-live-handle-position-channel.md
**Created:** 2026-02-28
**Total Tickets:** 3
**Estimated Total Complexity:** 6 (S=1, M=2, L=3 -> M + M + S = 2 + 2 + 1 = 5)

---

### Ticket 1: Add `position_tx` field to `LiveHandle`, `subscribe()` method, and `start_live()` wiring

**Description:**
Add `pub(crate) position_tx: Arc<tokio::sync::watch::Sender<u64>>` to the `LiveHandle` struct in
`src/live.rs`. Implement the `pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64>` method.
In `src/store.rs`, initialize `watch::channel(0u64)` inside `start_live()`, wrap the sender in
`Arc`, pass `Arc::clone(&position_tx)` into the spawned task, and populate the new field in the
`LiveHandle` literal. Update all bare `LiveHandle` struct literal constructions in both files'
`#[cfg(test)]` modules to add the new field with
`Arc::new(tokio::sync::watch::channel(0u64).0)`.

**Scope:**
- Modify: `src/live.rs` (struct definition, `subscribe()` method, all test `LiveHandle` literals)
- Modify: `src/store.rs` (`start_live()` body and `LiveHandle` literal; test `LiveHandle` literals
  in `live_projection_returns_state_when_live_active` and `projection_works_when_live_mode_active`)

**Acceptance Criteria:**
- [ ] `LiveHandle` struct has field `pub(crate) position_tx: Arc<tokio::sync::watch::Sender<u64>>`
  immediately after `caught_up`, with a doc comment matching the PRD specification.
- [ ] `pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64>` is implemented on `LiveHandle`
  by delegating to `self.position_tx.subscribe()`, with a full doc comment including the
  `# Usage` example from the PRD.
- [ ] `LiveHandle` still derives `Clone` without a manual `impl` (verified by `cargo build`).
- [ ] `start_live()` in `src/store.rs` creates `(position_tx, _position_rx)` via
  `tokio::sync::watch::channel(0u64)`, wraps `position_tx` in `Arc`, clones the `Arc` into the
  spawned task argument list (even though the task does not yet use it), and includes
  `position_tx` in the `LiveHandle { ... }` literal.
- [ ] All existing `LiveHandle` struct literals in test code in `src/live.rs` and `src/store.rs`
  compile with the new field added as
  `position_tx: Arc::new(tokio::sync::watch::channel(0u64).0)`.
- [ ] Test: construct a bare `LiveHandle` in a unit test (mirroring the existing pattern),
  call `subscribe()`, assert `*rx.borrow() == 0`.
  (`subscribe_returns_receiver_with_initial_value_zero`)
- [ ] Test: construct a bare `LiveHandle`, call `subscribe()` twice to get `rx1` and `rx2`,
  assert both start at 0; manually call `position_tx.send(42)`, assert `*rx1.borrow_and_update() == 42`
  and `*rx2.borrow_and_update() == 42`.
  (`subscribe_multiple_times_returns_independent_receivers`)
- [ ] Test: clone a `LiveHandle`, call `subscribe()` on both original and clone, assert
  `Arc::ptr_eq(&handle.position_tx, &cloned.position_tx)` is true, and that a send from
  the original's `position_tx` is visible on the clone's receiver.
  (`subscribe_on_cloned_handle_shares_same_channel`)
- [ ] `cargo build --locked` exits with code 0 and zero warnings.
- [ ] All pre-existing tests pass (`cargo test --locked`).

**Dependencies:** None
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 2, AC 4, AC 5, AC 6, AC 9, AC 10

---

### Ticket 2: Wire `position_tx` through `run_live_loop` and `process_stream_with_dispatch`, add position-send test

**Description:**
Update `run_live_loop` in `src/live.rs` to accept `position_tx: Arc<tokio::sync::watch::Sender<u64>>`
as a new parameter (mirroring the existing `caught_up: Arc<AtomicBool>` parameter). Pass a
reference `&position_tx` into each call to `process_stream_with_dispatch`. Update
`process_stream_with_dispatch` to accept `position_tx: &tokio::sync::watch::Sender<u64>` and,
after the PM dispatch loop for each `RecordedEvent`, emit
`let _ = position_tx.send(recorded.global_position);`. Update the call site in `src/store.rs`
where `run_live_loop` is spawned to pass `Arc::clone(&position_tx)`. Add the
`process_stream_sends_position_after_each_event` unit test.

**Scope:**
- Modify: `src/live.rs` (`run_live_loop` signature; `process_stream_with_dispatch` signature and
  body; all `process_stream_with_dispatch` call sites within `run_live_loop`; one new test;
  update existing test call sites for `process_stream_with_dispatch`)
- Modify: `src/store.rs` (`start_live()` — add `Arc::clone(&position_tx)` to the
  `run_live_loop` call inside the spawned closure)

**Acceptance Criteria:**
- [ ] `run_live_loop` signature gains `position_tx: Arc<tokio::sync::watch::Sender<u64>>` as a
  new parameter placed after `caught_up`, matching the doc comment parameter list.
- [ ] `process_stream_with_dispatch` signature gains `position_tx: &tokio::sync::watch::Sender<u64>`
  as a new parameter placed after `caught_up`.
- [ ] Inside `process_stream_with_dispatch`, after the final PM dispatch `for` loop (within the
  `subscribe_response::Content::Event` arm), `let _ = position_tx.send(recorded.global_position);`
  is present with a comment explaining the `let _` pattern (dropped send = all receivers gone).
- [ ] `CaughtUp` and `None` arms of `process_stream_with_dispatch` do NOT call `position_tx.send`.
- [ ] `start_live()` in `src/store.rs` passes `Arc::clone(&position_tx)` to the
  `run_live_loop(store_clone, caught_up_clone, Arc::clone(&position_tx), shutdown_rx)` call.
- [ ] All existing call sites of `process_stream_with_dispatch` in the `#[cfg(test)]` module are
  updated to pass a `watch::Sender` (created inline as
  `&tokio::sync::watch::channel(0u64).0`).
- [ ] Test: build a two-event stream (`event_response(0, 0)` and `event_response(1, 1)`) followed
  by `caught_up_response()`, create a `watch::Sender<u64>`, call
  `process_stream_with_dispatch(&store, stream, &caught_up, &position_tx)`, then assert
  `*position_tx.borrow() == 1` (the `global_position` of the second event).
  (`process_stream_sends_position_after_each_event`)
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits with code 0.
- [ ] `cargo test --locked` exits with all tests green.

**Dependencies:** Ticket 1
**Complexity:** M
**Maps to PRD AC:** AC 3, AC 6, AC 7, AC 8, AC 10

---

### Ticket 3: Verification and Integration Check

**Description:**
Run the complete PRD 012 acceptance criteria checklist. Confirm all four new tests listed in the
PRD Technical Approach are present and green, all pre-existing tests pass without assertion
changes, and no public API signatures changed.

**Acceptance Criteria:**
- [ ] `cargo build --locked` exits with code 0 and zero warnings.
- [ ] `cargo test --locked` exits with all tests green, including:
  - `subscribe_returns_receiver_with_initial_value_zero`
  - `subscribe_multiple_times_returns_independent_receivers`
  - `process_stream_sends_position_after_each_event`
  - `subscribe_on_cloned_handle_shares_same_channel`
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits with code 0.
- [ ] `cargo fmt --check` exits with code 0 (no formatting diffs).
- [ ] Grep confirms no public method signatures on `LiveHandle` or `AggregateStore` changed
  (run: `grep -n "pub fn " src/live.rs src/store.rs` and compare against pre-PR baseline).
- [ ] Grep confirms `watch::channel(0u64)` appears in `start_live()` in `src/store.rs`
  (run: `grep -n "watch::channel(0u64)" src/store.rs`).
- [ ] Grep confirms `position_tx.send(recorded.global_position)` appears in
  `src/live.rs` (run: `grep -n "position_tx.send" src/live.rs`).
- [ ] All PRD acceptance criteria (AC 1-10) pass end-to-end.
- [ ] No regressions in any module outside `src/live.rs` and `src/store.rs`.

**Dependencies:** Ticket 1, Ticket 2
**Complexity:** S
**Maps to PRD AC:** AC 1, AC 2, AC 3, AC 4, AC 5, AC 6, AC 7, AC 8, AC 9, AC 10

---

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1 | `LiveHandle` has `pub fn subscribe(&self) -> watch::Receiver<u64>` and compiles with `cargo build --locked` | Ticket 1, Ticket 3 | Covered |
| 2 | `handle.subscribe()` called before any events returns a receiver whose initial `*rx.borrow()` is `0` | Ticket 1, Ticket 3 | Covered |
| 3 | After `process_stream_with_dispatch` processes N events, `*rx.borrow_and_update()` equals the `global_position` of the Nth event | Ticket 2, Ticket 3 | Covered |
| 4 | `subscribe()` on two clones of the same `LiveHandle` yields receivers sharing the same underlying channel (verified via `Arc::ptr_eq`) | Ticket 1, Ticket 3 | Covered |
| 5 | `subscribe()` multiple times on the same instance yields independent `watch::Receiver` values observing the same updates | Ticket 1, Ticket 3 | Covered |
| 6 | Existing tests in `src/live.rs` and `src/store.rs` pass without modification to their assertions | Ticket 1, Ticket 2, Ticket 3 | Covered |
| 7 | `cargo clippy --all-targets --all-features --locked -- -D warnings` exits with code 0 | Ticket 2, Ticket 3 | Covered |
| 8 | `cargo test --locked` exits green, including the four new tests | Ticket 1, Ticket 2, Ticket 3 | Covered |
| 9 | No existing public method on `LiveHandle` or `AggregateStore` changes its signature | Ticket 1, Ticket 3 | Covered |
| 10 | `watch::Sender<u64>` is constructed unconditionally in `start_live()` regardless of `subscribe()` calls | Ticket 1, Ticket 2, Ticket 3 | Covered |
