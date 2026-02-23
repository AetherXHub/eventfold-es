# Code Review: Ticket 1 -- Add `list_aggregate_types` helper to `StreamLayout`

**Ticket:** 1 -- Add `list_aggregate_types` helper to `StreamLayout`
**Impl Report:** docs/prd/008-stream-enumeration-api-reports/ticket-01-impl.md
**Date:** 2026-02-22 12:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `list_aggregate_types()` compiles with `pub(crate)` visibility | Met | Line 205: `pub(crate) fn list_aggregate_types(...)`. Confirmed via `cargo build`. |
| 2 | Returns sorted names when streams exist (`["counter", "toggle"]`) | Met | Test `list_aggregate_types_returns_sorted_types` (line 386) creates `toggle` then `counter` in reverse order, asserts `vec!["counter", "toggle"]`. |
| 3 | Returns `Ok(vec![])` when `streams/` doesn't exist | Met | Test `list_aggregate_types_empty_when_no_streams_dir` (line 406) uses fresh TempDir, asserts empty result. Code at line 210 maps `NotFound` to `Ok(Vec::new())`. |
| 4 | Returns `Ok(vec![])` when `streams/` is empty | Met | No dedicated test, but logically covered: an empty `read_dir` iterator produces no entries, `filter_map` yields nothing, result is empty `Vec`. The `filter_map` only keeps directories, so even if `streams/` contained only files, result would be empty. Covered by logic, consistent with ticket scope of "2 unit tests". |
| 5 | Doc comment includes `# Returns` and `# Errors` | Met | Lines 195-203 contain both sections. The ticket also mentions `# Arguments` but the method takes no parameters beyond `&self`, making that section inapplicable. Omitting it is the correct Rust convention. The existing `list_streams` includes `# Arguments` because it accepts `aggregate_type: &str`. |
| 6 | `cargo test` passes | Met | Full suite: 105 unit tests + 8 doc-tests, 0 failures. `cargo clippy -- -D warnings` clean. `cargo fmt --check` clean. |

## Issues Found

### Critical (must fix before merge)
- None.

### Major (should fix, risk of downstream problems)
- None.

### Minor (nice to fix, not blocking)
- **`#[allow(dead_code)]` annotation (line 204):** Justified and documented in the impl report. The comment explains it will be consumed by Ticket 2. This is acceptable for a multi-ticket implementation. Ensure it is removed in Ticket 2.
- **`entry.ok()?` silently swallows I/O errors during `read_dir` iteration (line 216):** Both `entry.ok()?` (for the `DirEntry` result) and `entry.file_type().ok()?` (line 219) silently skip entries that fail to read. This is consistent with the existing `list_streams` method (line 254-262) which uses the identical pattern, so this is a codebase convention, not a defect. Noting for awareness.

## Suggestions (non-blocking)
- None. The implementation is clean, minimal, and follows the established `list_streams` pattern exactly.

## Scope Check
- Files within scope: YES -- only `src/storage.rs` was modified.
- Scope creep detected: NO -- exactly the method and 2 tests specified in the ticket.
- Unauthorized dependencies added: NO.

## Risk Assessment
- Regression risk: LOW -- The new method is `pub(crate)`, not yet called from anywhere, and guarded by `#[allow(dead_code)]`. No existing code paths are affected.
- Security concerns: NONE.
- Performance concerns: NONE -- filesystem scan is bounded by the number of aggregate type directories, which is expected to be small.
