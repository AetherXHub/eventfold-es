# Code Review: Ticket 2 -- Wire position_tx through run_live_loop and process_stream_with_dispatch

**Ticket:** 2 -- Wire position_tx through run_live_loop and process_stream_with_dispatch
**Impl Report:** docs/prd/012-live-handle-position-channel-reports/ticket-02-impl.md
**Date:** 2026-02-28 16:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `run_live_loop` gains `position_tx: Arc<tokio::sync::watch::Sender<u64>>` after `caught_up` | Met | `src/live.rs:271`. Parameter placed immediately after `caught_up` (line 270), matching field order in `LiveHandle` and the doc comment parameter list (lines 257-258). |
| 2 | `process_stream_with_dispatch` gains `position_tx: &tokio::sync::watch::Sender<u64>` after `caught_up` | Met | `src/live.rs:374`. Parameter placed after `caught_up` (line 373). Correct borrowing pattern -- function does not need ownership. |
| 3 | After PM dispatch loop, `let _ = position_tx.send(recorded.global_position);` with comment | Met | `src/live.rs:465-468`. Comment correctly explains the `let _` pattern: watch send only fails when all receivers are dropped. Placement is after all PM envelope dispatching (line 463 closes the dispatch `for` loop). |
| 4 | `CaughtUp` and `None` arms do NOT call `position_tx.send` | Met | Verified by inspection: `CaughtUp` arm (lines 470-472) only stores `caught_up` flag and logs. `None` arm (lines 474-476) only has a skip comment. No `position_tx` references in either. |
| 5 | `start_live()` passes `Arc::clone(&position_tx)` to `run_live_loop` | Met | `src/store.rs:243-245`. Replaces Ticket 1's `let _position_tx = position_tx_clone;` placeholder with actual parameter passing: `run_live_loop(store_clone, caught_up_clone, position_tx_clone, shutdown_rx)`. |
| 6 | All existing test call sites updated with `&tokio::sync::watch::channel(0u64).0` | Met | 6 call sites in `src/live.rs` tests updated: `live_loop_processes_events_and_fans_out_to_projections` (line 782), `live_loop_dispatches_pm_envelopes_dead_letters_unknown_type` (line 817), `live_loop_saves_checkpoints_on_shutdown` (line 855), `min_global_position_returns_min_of_projections_and_pms` (line 912), `stream_error_returns_stream_outcome_error` (line 959), `run_live_loop_shutdown_saves_checkpoints` (line 1017). Inline channel creation is correct -- receiver immediately dropped, which is fine since these tests don't observe position. |
| 7 | New test `process_stream_sends_position_after_each_event` | Met | `src/live.rs:983-1002`. Creates named `(position_tx, position_rx)` pair, builds two-event stream + CaughtUp, calls `process_stream_with_dispatch` with `&position_tx`, asserts `*position_rx.borrow() == 1` (global_position of second event). Test correctly validates the coalesced final value. Note: AC text says `*position_tx.borrow()` but `position_rx.borrow()` is equivalently correct (both `Sender::borrow()` and `Receiver::borrow()` return the current value). |
| 8 | `cargo clippy --all-targets --all-features --locked -- -D warnings` exits code 0 | Partial | 4 pre-existing `result_large_err` errors in `src/projection.rs` (lines 384, 394) and `src/process_manager.rs` (lines 541, 551) prevent clean exit. These are NOT introduced by this ticket -- confirmed identical on `main`. Lib-only clippy (`cargo clippy --lib --locked -- -D warnings`) passes clean. No new warnings introduced. |
| 9 | `cargo test --locked` exits with all tests green | Met | 126 unit tests + 7 doc-tests all pass. The new test `process_stream_sends_position_after_each_event` passes. Test count increased from 125 (Ticket 1) to 126. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- None

## Suggestions (non-blocking)
- The `process_stream_sends_position_after_each_event` test only asserts the final coalesced value (`*position_rx.borrow() == 1`). An intermediate assertion after processing just the first event would more thoroughly validate AC 3's "after each event" semantics. However, watch channels coalesce values, so the final-value assertion is sufficient to prove the send is happening at the right time (after PM dispatch, in the Event arm only). The test is correct as-is.
- The pre-existing `result_large_err` clippy warnings in `src/projection.rs` and `src/process_manager.rs` could be addressed in a separate housekeeping ticket by adding `#[allow(clippy::result_large_err)]` attributes (the same pattern already used in `src/live.rs` test helpers at lines 672, 686).

## Scope Check
- Files within scope: YES -- only `src/live.rs` and `src/store.rs` modified, exactly matching the ticket's listed file scope.
- Scope creep detected: NO -- all changes are strictly related to wiring `position_tx` through the two functions and updating call sites.
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- Changes are purely additive: new parameter threaded through existing functions, no logic changes to existing event processing or PM dispatch. All 126 tests pass. The `let _ =` discard pattern on `position_tx.send()` is correct and safe -- watch send only errors when all receivers are dropped, which is expected if no one subscribes.
- Security concerns: NONE
- Performance concerns: NONE -- `watch::Sender::send()` is O(1) and does not allocate. Called once per event, which is the minimal possible frequency.

## Verification
- `cargo build --locked`: PASS (zero warnings)
- `cargo test --locked`: PASS (126 unit + 7 doc-tests, all green)
- `cargo fmt --check`: PASS (no diffs)
- `cargo clippy --lib --locked -- -D warnings`: PASS (zero warnings)
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: 4 pre-existing `result_large_err` errors (not introduced by this ticket)
