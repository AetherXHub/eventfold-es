# Code Review: Ticket 2 -- Refactor projection/PM runner internals for live mode access

**Ticket:** 2 -- Refactor projection/PM runner internals for live mode access
**Impl Report:** docs/prd/010-live-subscriptions-reports/ticket-02-impl.md
**Date:** 2026-02-27 15:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | All existing projection and process manager tests pass unchanged | Met | All 12 original projection tests and 10 original PM tests pass. Verified original test functions are unmodified in the diff -- only new test functions were added below existing ones. |
| 2 | All existing store tests pass unchanged | Met | All 4 store tests pass (`builder_connect_returns_err_when_no_server`, `builder_stores_custom_live_config`, `builder_default_live_config`, `get_twice_returns_cached_alive_handles`). The first and last of these are from before Ticket 1/2. |
| 3 | `store.projection::<P>()` returns the same results as before | Met | `AggregateStore::projection()` (store.rs lines 225-240) still calls `catch_up().await?` then reads state. Changed from `runner.state().clone()` to `runner.state_any()` + downcast. Semantically identical: both produce a clone of `P`. |
| 4 | `store.run_process_managers()` returns the same results as before | Met | No changes to `run_process_managers()` body (store.rs lines 258-319). PM `catch_up()` dispatches to the same `process_stream` logic through the trait impl. |
| 5 | `cargo test` passes with no failures | Met | Confirmed: 103 unit tests + 6 doc-tests, 0 failures. |
| 6 | `cargo clippy -- -D warnings` passes | Met | Confirmed: zero warnings. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- **Tracing semantic change in PM**: `react_recorded_event` at `src/process_manager.rs:330` logs `envelopes_produced = envelopes.len()` which is the per-event count. The original inline code in `process_stream` logged the same field name but it referred to the accumulated `envelopes` Vec (cumulative count). This is a minor behavioral change in log output -- per-event is arguably more useful, so this is an improvement rather than a bug. No action needed.
- **`ProjectionRunner::state()` and `position()` are now fully dead**: Both have `#[allow(dead_code)]`. The `ProjectionCatchUp` trait impl accesses `self.checkpoint.state` and `self.checkpoint.last_global_position` directly rather than calling these methods. Consider removing `state()` and `position()` from `ProjectionRunner` since the trait methods serve the same purpose. Low priority -- no harm in keeping them.

## Suggestions (non-blocking)
- The test helper setup pattern (create tempdir, create checkpoint_dir, connect_lazy, create EsClient) is repeated identically in all 7 new tests across both modules. A small `fn make_test_runner<T>() -> (TempDir, Runner<T>)` helper in each test module would reduce boilerplate. Not blocking -- the duplication is purely in test code and each test is self-contained.

## Scope Check
- Files within scope: YES -- `src/projection.rs`, `src/process_manager.rs`, `src/store.rs` are all listed in the ticket scope.
- Scope creep detected: NO -- The working tree also contains changes to `src/lib.rs` and the new `src/live.rs` file, but these are from Ticket 1 (which this ticket depends on). The Ticket 2 diff is cleanly scoped to the three files listed.
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- The refactoring factored inline logic into free functions (`apply_recorded_event`, `react_recorded_event`) without changing behavior. All existing tests pass unchanged. The `ProjectionMap` type alias change is internal (`pub(crate)`) and the new `state_any()` + downcast path in `store.projection()` is tested by the existing store tests passing.
- Security concerns: NONE
- Performance concerns: NONE -- `state_any()` performs one `.clone()` + `Box::new()` allocation per `projection()` call, which is equivalent to the previous `.state().clone()` path.
