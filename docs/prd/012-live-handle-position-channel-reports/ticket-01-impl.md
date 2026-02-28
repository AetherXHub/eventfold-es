# Implementation Report: Ticket 1 -- Add position_tx field to LiveHandle, subscribe() method, and start_live() wiring

**Ticket:** 1 - Add position_tx field to LiveHandle, subscribe() method, and start_live() wiring
**Date:** 2026-02-28 12:00
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/live.rs` - Added `position_tx` field to `LiveHandle` struct with doc comment; added `subscribe()` method with full doc comment including Usage example; updated all 6 existing test `LiveHandle` struct literals to include the new field; added 3 new unit tests.
- `src/store.rs` - Updated `start_live()` to create `watch::channel(0u64)`, wrap sender in `Arc`, clone into spawned task, and populate `position_tx` in the `LiveHandle` literal; updated 2 test `LiveHandle` struct literals to include the new field.

## Implementation Notes
- The `position_tx` field is placed immediately after `caught_up` in the struct, matching the PRD specification for field ordering.
- The `Arc::clone(&position_tx)` is moved into the spawned task closure via a `let _position_tx = position_tx_clone;` binding, keeping the sender alive in the task scope even though the task does not yet use it (future ticket will wire it to `process_stream_with_dispatch`).
- The `subscribe()` method delegates directly to `self.position_tx.subscribe()`, which is the idiomatic tokio `watch` API for creating additional receivers from a sender.
- The doc comment on `subscribe()` includes a `no_run` example wrapped in hidden scaffolding (`# fn example(handle: eventfold_es::LiveHandle)` etc.) to make the doc-test compilable.
- All existing test `LiveHandle` literals use the inline pattern `Arc::new(tokio::sync::watch::channel(0u64).0)` to add the field with minimal noise.
- `LiveHandle` continues to derive `Clone` without a manual impl, since `Arc<watch::Sender<u64>>` is `Clone`.

## Acceptance Criteria
- [x] AC 1: `LiveHandle` struct has field `pub(crate) position_tx: Arc<tokio::sync::watch::Sender<u64>>` immediately after `caught_up`, with doc comment matching PRD specification - field added at line 93-95 of `src/live.rs`.
- [x] AC 2: `pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64>` implemented with full doc comment including `# Usage` example from PRD - method at line 112-139 of `src/live.rs`.
- [x] AC 3: `LiveHandle` still derives `Clone` without manual impl - `#[derive(Clone)]` remains at line 87, verified by `cargo build`.
- [x] AC 4: `start_live()` creates `(position_tx, _position_rx)` via `watch::channel(0u64)`, wraps in `Arc`, clones into task, populates `LiveHandle` literal - lines 237-256 of `src/store.rs`.
- [x] AC 5: All existing `LiveHandle` struct literals in test code compile with new field - all 6 in `src/live.rs` and 2 in `src/store.rs` updated.
- [x] AC 6: Test `subscribe_returns_receiver_with_initial_value_zero` - constructs bare `LiveHandle`, calls `subscribe()`, asserts `*rx.borrow() == 0`.
- [x] AC 7: Test `subscribe_multiple_times_returns_independent_receivers` - calls `subscribe()` twice, asserts both start at 0, sends 42, asserts both see 42.
- [x] AC 8: Test `subscribe_on_cloned_handle_shares_same_channel` - clones handle, asserts `Arc::ptr_eq`, subscribes on clone, sends 99 from original, asserts clone's receiver sees 99.
- [x] AC 9: `cargo build --locked` exits with code 0 and zero warnings.
- [x] AC 10: All pre-existing tests pass (`cargo test --locked`) - 125 unit tests + 7 doc tests all green.

## Test Results
- Lint: PASS (note: `cargo clippy --all-targets --all-features --locked -- -D warnings` has 4 pre-existing errors in `src/projection.rs` for `result_large_err` that exist on the main branch before this ticket; no new warnings introduced)
- Tests: PASS - 125 unit tests + 7 doc tests, all green
- Build: PASS - `cargo build --locked` exits 0, zero warnings
- Format: PASS - `cargo fmt --check` exits 0
- New tests added:
  - `src/live.rs` - `live::tests::subscribe_returns_receiver_with_initial_value_zero`
  - `src/live.rs` - `live::tests::subscribe_multiple_times_returns_independent_receivers`
  - `src/live.rs` - `live::tests::subscribe_on_cloned_handle_shares_same_channel`

## Concerns / Blockers
- Pre-existing clippy failure: `cargo clippy --all-targets --all-features --locked -- -D warnings` fails due to 4 `result_large_err` warnings in `src/projection.rs` (lines 381, 394) and `src/live.rs` (lines 666, 680) test helper functions. The `src/live.rs` ones already have `#[allow(clippy::result_large_err)]` but the `src/projection.rs` ones do not. This is not caused by this ticket's changes and exists identically on the `main` branch.
