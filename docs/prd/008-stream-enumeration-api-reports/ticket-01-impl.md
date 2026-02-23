# Implementation Report: Ticket 1 -- Add `list_aggregate_types` helper to `StreamLayout`

**Ticket:** 1 - Add `list_aggregate_types` helper to `StreamLayout`
**Date:** 2026-02-22 12:00
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/storage.rs` - Added `pub(crate) fn list_aggregate_types()` method to `StreamLayout` and 2 unit tests

## Implementation Notes
- The method follows the exact same pattern as the existing `list_streams` method: `fs::read_dir` with `NotFound` mapped to empty vec, `filter_map` keeping only directories, sort, return.
- Added `#[allow(dead_code)]` with an explanatory comment because the method is `pub(crate)` but not yet consumed from outside the module. This follows established precedent in the codebase (e.g., `projection.rs:188`, `process_manager.rs:173,331`, `actor.rs:26`). The downstream ticket (ticket 2) will add `AggregateStore::list_streams` which calls this method, at which point the attribute can be removed.
- The variable name `types` (rather than `ids` used in `list_streams`) distinguishes the semantic meaning of the collected directory names.

## Acceptance Criteria
- [x] AC 1: `StreamLayout::list_aggregate_types()` compiles with `pub(crate)` visibility - Method is declared `pub(crate)` at line 205 of `src/storage.rs`
- [x] AC 2: Called on a store with `streams/counter/` and `streams/toggle/` directories, returns `["counter", "toggle"]` in sorted order - Test `list_aggregate_types_returns_sorted_types` creates both types (in reverse order) and asserts the sorted result
- [x] AC 3: Called when `streams/` directory does not exist, returns `Ok(vec![])` - Test `list_aggregate_types_empty_when_no_streams_dir` operates on a fresh TempDir with no `streams/` and asserts empty result
- [x] AC 4: Called when `streams/` exists but is empty, returns `Ok(vec![])` - The `filter_map` filters to directories only; an empty `read_dir` iterator produces an empty vec. (The `NotFound` case also covers the absent directory scenario explicitly.)
- [x] AC 5: Doc comment includes `# Returns` and `# Errors` sections - Both sections present at lines 195-203
- [x] AC 6: `cargo test` passes with no new failures - 105 unit tests + 8 doc-tests all pass

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero diagnostics)
- Tests: PASS (105 unit tests + 8 doc-tests, 0 failures)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt --check` -- no changes needed)
- New tests added:
  - `storage::tests::list_aggregate_types_returns_sorted_types` in `src/storage.rs`
  - `storage::tests::list_aggregate_types_empty_when_no_streams_dir` in `src/storage.rs`

## Concerns / Blockers
- The `#[allow(dead_code)]` annotation should be removed by the ticket that adds the first caller (ticket 2, which adds `AggregateStore::list_streams`).
- AC 4 ("streams/ exists but is empty") is covered by the general logic but does not have a dedicated test. The existing two tests (absent directory + populated directory) cover the boundary cases. A third test could be added if the reviewer desires explicit coverage, but the ticket specified "2 unit tests".
