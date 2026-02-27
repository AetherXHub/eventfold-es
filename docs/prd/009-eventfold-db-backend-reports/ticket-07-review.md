# Code Review: Ticket 7 -- src/aggregate.rs: remove JSONL bridge functions

**Ticket:** 7 -- src/aggregate.rs: remove JSONL bridge functions; update tests
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-07-impl.md
**Date:** 2026-02-27 14:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `reducer()`, `reduce<A>()`, `to_eventfold_event()` deleted from aggregate.rs | Met | Confirmed via grep -- zero matches for these function names in `src/aggregate.rs`. File contains only the `Aggregate` trait (lines 1-49), `test_fixtures` (lines 51-117), and 8 unit tests (lines 119-186). |
| 2 | All eventfold crate imports removed from aggregate.rs | Met | Only import is `use serde::{Serialize, de::DeserializeOwned}` (line 3). Grep for `use eventfold` returns zero matches in `src/aggregate.rs`. |
| 3 | lib.rs no longer exports `reducer` or `to_eventfold_event` | Met | `src/lib.rs` line 24: `pub use aggregate::Aggregate` is the sole re-export from the aggregate module. Grep for `reducer` and `to_eventfold_event` in lib.rs returns zero matches. |
| 4 | Tests referencing removed functions are gone | Met | Grep for `reducer`, `to_eventfold_event`, `eventfold::Event`, `eventfold::ReduceFn` in `src/aggregate.rs` returns zero matches. All 8 remaining tests reference only `Aggregate`, `Counter`, `CounterCommand`, `CounterError`, `CounterEvent`. |
| 5 | Remaining Aggregate trait tests preserved and passing | Met | All 8 tests present: `handle_increment`, `handle_decrement_nonzero`, `handle_decrement_at_zero`, `handle_add`, `apply_incremented`, `apply_decremented`, `apply_added`, `handle_then_apply_roundtrip`. All pass in `cargo test` output (62 tests total, 8 in `aggregate::tests`). |
| 6 | test_fixtures module retained as pub(crate) | Met | Lines 51-117: `#[cfg(test)] pub(crate) mod test_fixtures` contains `Counter`, `CounterCommand`, `CounterError`, `CounterEvent` -- all `pub(crate)`. |
| 7 | Test: `Counter::handle(Increment)` returns `vec![Incremented]` | Met | `handle_increment` test at line 125: asserts `events == vec![CounterEvent::Incremented]`. Passes. |
| 8 | Test: `Counter::apply(Incremented)` returns `Counter { value: 1 }` | Met | `apply_incremented` test at line 159: asserts `counter.value == 1` after applying `Incremented` to a default `Counter`. Passes. |
| 9 | Quality gates pass | Met | `cargo build` -- success. `cargo test` -- 62 tests + 3 doc-tests all pass. `cargo clippy -- -D warnings` -- clean. `cargo fmt --check` -- clean. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- **Stale references in `src/actor.rs`**: The commented-out actor module (`// mod actor;` in lib.rs) still contains `use crate::aggregate::{Aggregate, reducer, to_eventfold_event}` at line 18 and references to both functions at lines 186 and 349. This is dead code (not compiled) and the impl report correctly notes it will be cleaned up in Ticket 8 (actor rewrite). No action needed for this ticket.
- **Impl report inaccuracy**: The report states "CANNOT RUN" for build/test/clippy due to "pre-existing compilation errors in `src/client.rs` and `src/snapshot.rs`". In practice, all four quality gates pass cleanly (62 tests, zero warnings, zero format issues). These compilation issues appear to have been resolved by sibling tickets before this review ran. The report was accurate at time of writing but is misleading now.

## Suggestions (non-blocking)
- None. The implementer correctly identified that Ticket 2 had already performed this work and verified all ACs rather than making redundant changes.

## Scope Check
- Files within scope: YES -- `src/aggregate.rs` and `src/lib.rs` are both in scope per ticket definition. No modifications were needed (work done by Ticket 2).
- Scope creep detected: NO
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- No code changes were made by this ticket. The JSONL bridge functions were already removed by Ticket 2. All existing tests pass.
- Security concerns: NONE
- Performance concerns: NONE
