# Code Review: Ticket 3 -- LiveHandle and live subscription loop

**Ticket:** 3 -- LiveHandle and live subscription loop
**Impl Report:** docs/prd/010-live-subscriptions-reports/ticket-03-impl.md
**Date:** 2026-02-27 17:00
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| AC 14 (import) | `LiveHandle` is publicly importable from `eventfold_es::LiveHandle` | Met | `pub use live::{LiveConfig, LiveHandle};` in `src/lib.rs` line 32. `LiveHandle` struct is `pub` at `src/live.rs` line 88. |
| AC 8 | `LiveHandle::shutdown()` saves final checkpoints and the task exits | Met | `shutdown()` at lines 126-141 sends shutdown signal then awaits the task. The `run_live_loop` function (lines 248-252, 289-292) calls `save_all_checkpoints` on shutdown signal. Tests `run_live_loop_shutdown_saves_checkpoints` (line 846) and `live_loop_saves_checkpoints_on_shutdown` (line 718) verify checkpoint persistence. |
| AC 3 | `LiveHandle::is_caught_up()` returns `true` after `CaughtUp` received | Met | `is_caught_up()` at line 105 reads `caught_up` atomic with `Ordering::Acquire`. `CaughtUp` sentinel handling at line 432 stores `true` with `Ordering::Release`. Tests `is_caught_up_returns_false_initially` (line 470) and `is_caught_up_returns_true_after_flag_set` (line 481) verify both states. Test `live_loop_processes_events_and_fans_out_to_projections` (line 672) verifies the flag is set after processing a stream with CaughtUp. |
| AC 4/5 | The loop processes events and fans out to projections | Met | `process_stream_with_dispatch` (lines 336-443) iterates stream events, applies to all projections (lines 355-361), reacts with all PMs (lines 364-375). Test `live_loop_processes_events_and_fans_out_to_projections` (line 654) verifies 2 events produce count=2 in EventCounter projection via `state_any()` downcast. |
| AC 6 | PM envelopes are dispatched during the loop | Met | Lines 378-429 dispatch envelopes through `store.dispatchers`, using the same pattern as `AggregateStore::run_process_managers()`. Test `live_loop_dispatches_pm_envelopes_dead_letters_unknown_type` (line 688) verifies EchoSaga produces an envelope and dispatch is attempted. |
| AC 7 | Dead-lettered envelopes go to per-PM log files | Met | Lines 411-426 dead-letter on unknown aggregate type; lines 396-407 dead-letter on dispatch failure. Both call `append_dead_letter` with the PM's `dead_letter_path()`. Test (line 704-714) reads `dead_letters.jsonl` and confirms "unknown aggregate type: target" content. |
| AC 9 | Exponential backoff on reconnect respects `LiveConfig` values | Met | Backoff logic: initial = `config.reconnect_base_delay` (line 242), doubled on failure capped at `config.reconnect_max_delay` (lines 271, 317), reset to base on clean EOF after caught-up (line 304). Backoff waits interruptible by shutdown (lines 264-270, 311-315). Test `backoff_respects_live_config_values` (line 790) verifies doubling and cap. |
| AC 14 (docs) | `LiveHandle` and all public methods have doc comments | Met | `LiveHandle` struct (lines 79-96), `is_caught_up` (lines 99-107), `shutdown` (lines 109-141) all have comprehensive doc comments. `LiveConfig` docs were verified in Ticket 1 review. Internal functions `save_all_checkpoints`, `min_global_position`, `run_live_loop`, `process_stream_with_dispatch` also documented. |
| - | `cargo test` passes | Met | Verified: 116 unit tests + 6 doc-tests, 0 failures. |
| - | `cargo clippy -- -D warnings` passes | Met | Verified: zero warnings on non-test code. |
| - | `cargo fmt --check` passes | Met | Verified: no diffs. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)

1. **Unnecessary `#[allow(dead_code)]` on `StreamOutcome` enum** (`src/live.rs` line 146): The annotation reads "Variants accessed by pattern match; Error field used for logging." I verified that removing this annotation produces zero warnings -- both variants and the inner `io::Error` field are genuinely used (destructured in match arms at lines 298 and 307). This annotation can be removed.

2. **`caught_up` flag is never reset to `false`** (`src/live.rs` line 432): The `AtomicBool` is set to `true` when `CaughtUp` is received but never reset on disconnect/reconnect. This means `is_caught_up()` returns `true` even while the loop is mid-reconnect with potential data lag. The backoff reset at line 302-304 also relies on this stale flag (resets backoff after any clean EOF if CaughtUp was EVER seen, even on a previous connection). The implementer documented this as "functionally equivalent" in the impl report. This is a design choice rather than a bug -- "has ever caught up" vs "is currently caught up" -- but downstream consumers should be aware that `is_caught_up() == true` does not mean the live loop is connected.

3. **`#[allow(dead_code)]` annotations in `projection.rs` and `process_manager.rs` are now stale**: The implementer correctly notes (impl report Concerns section) that `apply_event`, `position`, and `save` on `ProjectionCatchUp` and `react_event`, `position` on `ProcessManagerCatchUp` have `#[allow(dead_code)]` annotations with comments "Will be consumed by the live subscription loop." This ticket's code now consumes them. Left as-is since those files are outside ticket scope -- this is correct behavior; should be cleaned up in Ticket 5 (verification pass) or a future pass.

## Suggestions (non-blocking)

- The `envelope.clone()` at line 383 (dispatch path) is necessary and actually more efficient than the `store.rs::run_process_managers` pattern which clones in both the dispatch AND dead-letter paths. The live.rs version clones only for dispatch, keeping the original for dead-lettering. Nice optimization.

- The `save_all_checkpoints` function logs warnings but does not propagate errors (lines 164-185). This is correct for the live loop context (partial checkpoint failure should not crash the loop), but `shutdown()` callers might want to know if the final checkpoint save failed. Currently `shutdown` returns `Ok(())` even if checkpoint saves partially failed. This is an acceptable tradeoff since the alternative (propagating partial errors) would complicate the API, but worth documenting.

- Test helper functions (`mock_client`, `mock_store_with_projection`, `mock_store_with_projection_and_pm`, `make_recorded_event`, `event_response`, `caught_up_response`) duplicate patterns from `process_manager::tests` and `store::tests`. Consolidating into a shared `test_utils` module would reduce boilerplate. Not blocking as this is a common pattern in the codebase.

## Scope Check
- Files within scope: PARTIAL -- `src/live.rs` and `src/lib.rs` are within scope per the ticket definition. `src/store.rs` is NOT listed in the ticket scope.
- Scope creep detected: YES (minor) -- The `src/store.rs` changes (making `AggregateStore` fields, `AggregateDispatcher` trait, and `DispatcherMap` type alias `pub(crate)`) are outside the listed ticket scope. However, these are architecturally necessary for `run_live_loop` to access store internals and replicate the dispatch pattern. The changes are purely visibility changes (no behavioral modification), and the implementer explicitly documented this deviation with justification. The alternative (adding accessor methods) would have been more code for no additional safety since these are `pub(crate)`. Given that the changes are minimal, documented, and mechanically necessary, this does not warrant blocking the review.
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- The `run_live_loop` function is `#[allow(dead_code)]` and not yet called from any production path (wiring happens in Ticket 4). The `pub(crate)` visibility expansion in `store.rs` does not change behavior. All 116 existing tests continue to pass.
- Security concerns: NONE
- Performance concerns: NONE -- The live loop processes events sequentially with brief lock acquisitions. No unbounded allocations, no blocking operations on the async reactor. Checkpoint saves are periodic (configurable via `LiveConfig`), not per-event.
