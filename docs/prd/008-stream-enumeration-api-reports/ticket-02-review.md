# Code Review: Ticket 2 -- Implement `list_streams` and `read_events` on `AggregateStore`

**Ticket:** 2 -- Implement `list_streams` and `read_events` on `AggregateStore`
**Impl Report:** docs/prd/008-stream-enumeration-api-reports/ticket-02-impl.md
**Date:** 2026-02-22 17:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `list_streams(None)` returns sorted `(type, id)` pairs across all types | Met | Test `list_streams_none_returns_all_sorted` (line 1846) creates counter c-1, c-2 and toggle t-1, asserts exact order `[("counter","c-1"),("counter","c-2"),("toggle","t-1")]`. Implementation at lines 508-518 iterates sorted types from `list_aggregate_types()`, then sorted IDs from `list_streams()`, producing naturally sorted output. |
| 2 | `list_streams(Some("counter"))` returns only counter pairs | Met | Test `list_streams_some_filters_by_type` (line 1873) creates counter+toggle streams, filters by "counter", asserts `[("counter","c-1"),("counter","c-2")]`. Implementation at lines 499-507 delegates to `layout.list_streams()` for the single type. |
| 3 | `list_streams(None)` on empty store returns `Ok(vec![])` | Met | Test `list_streams_none_empty_store` (line 1898) uses fresh store, asserts `pairs.is_empty()`. Covered by `list_aggregate_types()` returning empty vec when `streams/` dir doesn't exist. |
| 4 | `list_streams(Some("nonexistent"))` returns `Ok(vec![])` | Met | Test `list_streams_some_nonexistent_type` (line 1913) asserts `pairs.is_empty()`. Covered by `layout.list_streams()` returning empty vec for non-existent type directory. |
| 5 | `read_events` after commands returns correct event count | Met | Test `read_events_returns_all_events` (line 1928) executes 2 increments, asserts `events.len() == 2` and both have `event_type == "Incremented"`. |
| 6 | `read_events` on stream with no commands returns `Ok(vec![])` | Met | Test `read_events_empty_stream_returns_empty_vec` (line 1948) calls `store.get::<Counter>("c-1")` to create dir without commands, asserts `events.is_empty()`. Implementation at line 568 maps `NotFound` from `read_from(0)` (no `app.jsonl`) to `Ok(Vec::new())`. |
| 7 | `read_events` on non-existent stream returns `Err(NotFound)` | Met | Test `read_events_nonexistent_stream_returns_not_found` (line 1972) asserts `result.is_err()` and `kind() == NotFound`. Implementation at lines 556-561 checks `stream_dir.is_dir()` and returns explicit `NotFound`. |
| 8 | Both methods have doc comments with `# Arguments`, `# Returns`, `# Errors` | Met | `list_streams` doc: lines 473-492 (summary, `# Arguments`, `# Returns`, `# Errors`). `read_events` doc: lines 522-543 (summary, `# Arguments`, `# Returns`, `# Errors`). Both match the style of existing `get` and `list` methods. |
| 9 | `cargo test` passes | Met | Full suite: 112 unit tests + 8 doc-tests, 0 failures. `cargo clippy -- -D warnings` clean. `cargo fmt --check` clean. |

## Issues Found

### Critical (must fix before merge)
- None.

### Major (should fix, risk of downstream problems)
- None.

### Minor (nice to fix, not blocking)
- **Duplicate `Toggle` fixture types (lines 1073 vs 1804):** The `Toggle`/`ToggleEvent`/`ToggleError` types are defined twice in the test module -- once inside the `two_aggregate_types_coexist` function body (line 1073, pre-existing) and once at module level (line 1804, new). Both compile because they are in different scopes. The module-level definition is shared by the new `list_streams` tests and the `toggle()` helper. This is not a defect but creates minor confusion. Could be unified by removing the function-scoped copy and updating `two_aggregate_types_coexist` to use the module-level type, but that modifies pre-existing test code and is purely cosmetic.
- **`#[allow(dead_code)]` on `list_aggregate_types` in `src/storage.rs` (line 204):** Now that Ticket 2 consumes this method, the annotation is no longer needed. However, removing it requires modifying `src/storage.rs` which is outside this ticket's scope (`src/store.rs` only). The implementer correctly noted this. Should be cleaned up in Ticket 3 or a future pass.
- **`agg_type.clone()` inside iterator (line 503):** In the `Some` branch, `agg_type.clone()` is called for every ID in the `into_iter().map()`. Since `agg_type` is moved into the closure and needs to be cloned per tuple, this is unavoidable without restructuring. No performance concern for realistic stream counts.

## Suggestions (non-blocking)
- In `list_streams` `None` branch (line 510), `Vec::with_capacity` could be used if a rough size estimate were available (e.g., sum of `ids.len()` per type), but given CRM-scale data volumes, this is negligible.
- The `read_events` closure (lines 572-577) could use `iter.map(|r| r.map(|(event, _, _)| event)).collect::<io::Result<Vec<_>>>()` instead of the explicit loop, but the loop is perfectly clear and consistent with imperative style elsewhere.

## Scope Check
- Files within scope: YES -- only `src/store.rs` was modified, exactly as specified.
- Scope creep detected: NO -- exactly two methods and seven tests added, matching the ticket description.
- Unauthorized dependencies added: NO.

## Risk Assessment
- Regression risk: LOW -- Both methods are strictly additive. No existing method signatures, behavior, or on-disk format was modified. All 112 pre-existing tests continue to pass.
- Security concerns: NONE.
- Performance concerns: NONE -- `list_streams(None)` performs one `read_dir` per aggregate type plus one for the `streams/` directory. `read_events` reads the full JSONL file sequentially. Both are bounded by the number of streams/events, which is appropriate for the use case (sync engine enumeration).
