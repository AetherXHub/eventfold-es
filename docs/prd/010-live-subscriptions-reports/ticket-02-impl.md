# Implementation Report: Ticket 2 -- Refactor projection/PM runner internals for live mode access

**Ticket:** 2 - Refactor projection/PM runner internals for live mode access
**Date:** 2026-02-27 14:30
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/projection.rs` - Added `ProjectionCatchUp` type-erased trait with `catch_up`, `apply_event`, `position`, `save`, `state_any` methods. Factored decode+apply logic from `process_stream` into shared `apply_recorded_event` helper. Implemented `ProjectionCatchUp` for `ProjectionRunner<P>`. Added 4 new tests.
- `src/process_manager.rs` - Added `react_event` and `position` methods to existing `ProcessManagerCatchUp` trait. Factored decode+react logic from `process_stream` into shared `react_recorded_event` helper. Implemented new trait methods for `ProcessManagerRunner<PM>`. Added 3 new tests.
- `src/store.rs` - Changed `ProjectionMap` type alias from `HashMap<String, Box<dyn Any + Send + Sync>>` to `HashMap<String, tokio::sync::Mutex<Box<dyn ProjectionCatchUp>>>`. Updated `ProjectionFactory` type alias accordingly. Updated `AggregateStoreBuilder::projection::<P>()` factory to produce `Box<dyn ProjectionCatchUp>`. Updated `AggregateStore::projection::<P>()` to use `state_any()` + downcast instead of `dyn Any` downcast of the runner.

## Implementation Notes

- **Shared helpers**: Both `apply_recorded_event` (projection) and `react_recorded_event` (process manager) are free functions that factor the decode+apply/react logic out of `process_stream`. This ensures the batch catch-up path and the single-event live path use identical logic with no duplication.
- **`EnteredSpan` is `!Send`**: The original `process_stream` in `projection.rs` used `tracing::debug_span!(...).entered()` which held a `!Send` guard across `.await` points. This prevented the boxed future from being `Send`. Replaced with a simple `tracing::debug!` call at the start of the method, consistent with the process manager's existing pattern.
- **`#[allow(dead_code)]` on new trait methods**: The new `apply_event`, `position`, and `save` methods on `ProjectionCatchUp`, and `react_event` and `position` on `ProcessManagerCatchUp`, are annotated with `#[allow(dead_code)]` since they will be consumed by the live subscription loop in Tickets 3/4. The `catch_up` and `state_any` methods on `ProjectionCatchUp` are already consumed by `AggregateStore::projection::<P>()`.
- **No `dyn Any` downcast in store projection access**: The previous approach downcast a `Box<dyn Any>` to `tokio::sync::Mutex<ProjectionRunner<P>>`. The new approach stores `tokio::sync::Mutex<Box<dyn ProjectionCatchUp>>` directly, calls `state_any()` on the trait object, and downcasts the returned `Box<dyn Any>` to `P`. This is cleaner and eliminates the double-indirection of the previous `dyn Any` wrapping.
- **Test pattern**: New tests use `EsClient::from_inner` with a lazy channel (requires tokio runtime), so they are `#[tokio::test]` rather than `#[test]`.

## Acceptance Criteria
- [x] AC 1: All existing projection and process manager tests pass unchanged -- all 12 original projection tests and 10 original PM tests pass without modification.
- [x] AC 2: All existing store tests pass unchanged -- all 4 store tests pass without modification.
- [x] AC 3: `store.projection::<P>()` returns the same results as before the refactor -- implementation uses `state_any()` + downcast, producing identical results.
- [x] AC 4: `store.run_process_managers()` returns the same results as before -- no changes to the run_process_managers code path; existing tests pass.
- [x] AC 5: `cargo test` passes with no failures -- 103 unit tests + 6 doc-tests all pass.
- [x] AC 6: `cargo clippy -- -D warnings` passes -- zero warnings.

## Test Results
- Lint: PASS (cargo clippy -- -D warnings: zero warnings)
- Tests: PASS (103 unit tests + 6 doc-tests, 0 failures)
- Build: PASS (cargo build: zero warnings)
- Format: PASS (cargo fmt --check: no diffs)
- New tests added:
  - `src/projection.rs`: `projection_catch_up_apply_event_decodes_and_advances_position`, `projection_catch_up_position_starts_at_zero`, `projection_catch_up_state_any_returns_cloned_state`, `projection_catch_up_save_persists_checkpoint`
  - `src/process_manager.rs`: `pm_catch_up_react_event_decodes_and_advances_position`, `pm_catch_up_position_starts_at_zero`, `pm_catch_up_react_event_skips_non_es_events`

## Concerns / Blockers
- None
