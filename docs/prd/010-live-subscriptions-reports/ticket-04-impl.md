# Implementation Report: Ticket 4 -- AggregateStore start_live() and live_projection()

**Ticket:** 4 - AggregateStore start_live() and live_projection()
**Date:** 2026-02-27 14:30
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/store.rs` - Added `live_handle` field to `AggregateStore`, `start_live()` and `live_projection()` methods, updated `mock_store` helpers and builder `open()` to include new field, added 6 new tests.
- `src/live.rs` - Changed `LiveHandle` fields from private to `pub(crate)` so `store.rs` can construct the struct directly. Updated 3 test helper functions that construct `AggregateStore` to include the new `live_handle` field. Removed `#[allow(dead_code)]` from `run_live_loop` since it is now called from `start_live()`.

## Implementation Notes
- `live_handle` field is `Arc<tokio::sync::Mutex<Option<LiveHandle>>>`, initialized as `None`. This matches the ticket spec exactly.
- `start_live()` follows the 8-step process outlined in the ticket: lock mutex, check AlreadyExists, create watch channel + AtomicBool, clone store, spawn tokio task running `run_live_loop`, build LiveHandle, store in mutex, return clone.
- `live_projection()` checks the `live_handle` mutex: if Some (live active), reads projection state via `state_any()` + downcast without catch-up; if None, falls back to `self.projection::<P>()`.
- `LiveHandle` fields were changed from private to `pub(crate)` because `start_live()` in `store.rs` needs to construct the struct. This is a minimal visibility change within the same crate.
- The `#[allow(dead_code)]` on `run_live_loop` was removed since it is now called from `start_live()`.
- Both `start_live` and `live_projection` have full doc comments documenting arguments, returns, and errors.
- The `live.rs` test helpers that construct `AggregateStore` directly needed the new field -- this is a mechanical necessity, not scope creep.

## Acceptance Criteria
- [x] AC 1: `store.start_live().await` returns `Ok(handle)` - Tested in `start_live_returns_ok_handle`
- [x] AC 2: Second `store.start_live().await` returns `Err` with `ErrorKind::AlreadyExists` - Tested in `start_live_twice_returns_already_exists_error`
- [x] AC 4/5: `store.live_projection::<P>()` returns state without catch-up - Tested in `live_projection_returns_state_when_live_active` (manually applies 3 events via `apply_event`, verifies count=3 without catch-up)
- [x] AC 12: `store.live_projection::<P>()` falls back to pull-based catch-up when live mode is not active - Tested in `live_projection_falls_back_when_not_live`
- [x] AC 12: `store.projection::<P>()` still works whether or not live mode is active - Tested in `projection_works_when_live_mode_active` and `projection_works_when_live_mode_not_active`
- [x] AC 14: `start_live` and `live_projection` have doc comments - Both methods have full doc comments following project patterns
- [x] `cargo test` passes - 122 unit tests + 6 doc-tests all pass
- [x] `cargo clippy -- -D warnings` passes - Clean

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` clean; `cargo clippy --tests -- -D warnings` has 4 pre-existing errors in `projection.rs` and `process_manager.rs` -- not introduced by this ticket)
- Tests: PASS (122 unit tests + 6 doc-tests, all pass)
- Build: PASS (zero warnings)
- Fmt: PASS (`cargo fmt --check` clean)
- New tests added:
  - `src/store.rs::tests::start_live_returns_ok_handle`
  - `src/store.rs::tests::start_live_twice_returns_already_exists_error`
  - `src/store.rs::tests::live_projection_returns_state_when_live_active`
  - `src/store.rs::tests::live_projection_falls_back_when_not_live`
  - `src/store.rs::tests::projection_works_when_live_mode_active`
  - `src/store.rs::tests::projection_works_when_live_mode_not_active`

## Concerns / Blockers
- `src/live.rs` was modified (3 test helpers + field visibility + dead_code removal) even though the ticket scope only lists `src/store.rs`. This was mechanically required: (1) `LiveHandle` fields must be `pub(crate)` for `start_live()` to construct it, (2) test helpers that construct `AggregateStore` directly must include the new field, and (3) `run_live_loop` is no longer dead code. These are minimal, necessary changes.
- Pre-existing clippy `result_large_err` warnings exist in `projection.rs` and `process_manager.rs` test code (4 instances). These are not introduced by this ticket and only show with `--tests` flag. The non-test `cargo clippy -- -D warnings` passes clean.
