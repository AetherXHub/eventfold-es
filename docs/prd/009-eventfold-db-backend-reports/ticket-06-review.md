# Code Review: Ticket 6 -- src/storage.rs: narrow to snapshot directory helpers only

**Ticket:** 6 -- src/storage.rs: narrow to snapshot directory helpers only
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-06-impl.md
**Date:** 2026-02-27 19:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | StreamLayout retains only: new(), base_dir(), projections_dir(), process_managers_dir(), snapshots_dir() | Met | Verified in `src/storage.rs` lines 32-75: exactly these 5 methods exist, no others. |
| 2 | All JSONL-era methods removed | Met | Diff confirms removal of `stream_dir`, `views_dir`, `ensure_stream`, `list_streams`, `list_aggregate_types`, `meta_dir`, and all registry file logic. All `std::fs`, `std::io`, `std::time` imports also removed. |
| 3 | StreamLayout visibility changed to pub(crate) | Met | Line 28: `pub(crate) struct StreamLayout`. All 5 methods are `pub(crate)`. |
| 4 | Tests updated for remaining methods only | Met | 8 old JSONL-related tests removed. 4 new tests covering the 4 path-returning methods. |
| 5 | Test: layout.snapshots_dir() returns `<base_dir>/snapshots` | Met | `snapshots_dir_returns_base_dir_slash_snapshots` test at line 91. |
| 6 | Test: layout.projections_dir() returns `<base_dir>/projections` | Met | `projections_dir_returns_base_dir_slash_projections` test at line 98. |
| 7 | Test: layout.process_managers_dir() returns `<base_dir>/process_managers` | Met | `process_managers_dir_returns_base_dir_slash_process_managers` test at line 105. |
| 8 | Quality gates pass | Met | `cargo fmt --check` clean, `cargo clippy -- -D warnings` zero warnings, `cargo test` 81 unit + 3 doc-tests all pass. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- None

## Suggestions (non-blocking)
- The `#![allow(dead_code)]` inner attribute at line 3 is appropriate for the transitional state where no consumer exists yet. The comment explaining why is good. This should be removed when Ticket 11 (store.rs rewrite) wires up `StreamLayout` as a consumer.

## Scope Check
- Files within scope: YES -- `src/storage.rs` and `src/lib.rs` are the only files listed in the ticket scope, and the impl report claims changes to only these two files. Since all PRD-009 tickets share an uncommitted working tree, the `src/lib.rs` diff against HEAD includes changes from Tickets 1-5 and 7, but the Ticket 6-specific change (uncommenting `mod storage;` and removing `pub use storage::StreamLayout`) is verified present and correct.
- Scope creep detected: NO
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- This is a pure simplification removing unused code. The 4 remaining path helpers are trivial `PathBuf::join` calls. No consumers of the removed JSONL methods exist in the current working tree (those call sites were already removed or will be removed by other tickets).
- Security concerns: NONE
- Performance concerns: NONE
