# Implementation Report: Ticket 2 -- Implement `list_streams` and `read_events` on `AggregateStore`

**Ticket:** 2 - Implement `list_streams` and `read_events` on `AggregateStore`
**Date:** 2026-02-22 16:30
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/store.rs` - Added `list_streams` and `read_events` public async methods to `impl AggregateStore`, plus 7 unit tests and shared Toggle test fixture

## Implementation Notes
- Both methods follow the existing `list<A>` pattern: clone `self.layout`, own all data needed by the closure, and delegate to `tokio::task::spawn_blocking` for blocking filesystem I/O.
- `list_streams(None)` calls `layout.list_aggregate_types()` (added by Ticket 1) then `layout.list_streams()` for each type. Because both return sorted vectors and we iterate types in sorted order, the output is naturally sorted by (aggregate_type, instance_id) without an additional sort pass.
- `list_streams(Some(agg_type))` delegates directly to `layout.list_streams()` and maps to `(agg_type, id)` pairs.
- `read_events` first checks if the stream directory exists (returning `NotFound` if not), then uses `EventReader::new` + `read_from(0)`. The `read_from` returning `NotFound` (no `app.jsonl` yet) is mapped to `Ok(vec![])`, consistent with the pattern in `ProjectionRunner::catch_up`.
- The `#[allow(dead_code)]` annotation on `list_aggregate_types` in `src/storage.rs` is now technically unnecessary since it has a consumer, but removing it is outside the ticket's file scope. It causes no warnings or issues.
- Test fixture types (Toggle, ToggleEvent, ToggleError) are defined at the test module level to avoid duplication across tests. The existing inline Toggle in `two_aggregate_types_coexist` is in a different (function) scope and does not conflict.

## Acceptance Criteria
- [x] AC 1: `list_streams(None)` on store with counter `["c-1","c-2"]` and toggle `["t-1"]` returns `[("counter","c-1"),("counter","c-2"),("toggle","t-1")]` -- verified by `list_streams_none_returns_all_sorted` test
- [x] AC 2: `list_streams(Some("counter"))` returns only counter pairs -- verified by `list_streams_some_filters_by_type` test
- [x] AC 3: `list_streams(None)` on empty store returns `Ok(vec![])` -- verified by `list_streams_none_empty_store` test
- [x] AC 4: `list_streams(Some("nonexistent"))` returns `Ok(vec![])` -- verified by `list_streams_some_nonexistent_type` test
- [x] AC 5: `read_events("counter","c-1")` after two Increment commands returns Vec of length 2 -- verified by `read_events_returns_all_events` test (also checks `event_type == "Incremented"`)
- [x] AC 6: `read_events` on stream with directory but no commands returns `Ok(vec![])` -- verified by `read_events_empty_stream_returns_empty_vec` test
- [x] AC 7: `read_events("nonexistent","x")` returns `Err` with `NotFound` -- verified by `read_events_nonexistent_stream_returns_not_found` test
- [x] AC 8: Both methods have doc comments with `# Arguments`, `# Returns`, `# Errors` -- present on both `list_streams` and `read_events`
- [x] AC 9: `cargo test` passes with no new failures -- 112 unit tests + 8 doc-tests all pass

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero diagnostics)
- Tests: PASS (112 unit tests + 8 doc-tests, 0 failures)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt` -- clean)
- New tests added:
  - `store::tests::list_streams_none_returns_all_sorted`
  - `store::tests::list_streams_some_filters_by_type`
  - `store::tests::list_streams_none_empty_store`
  - `store::tests::list_streams_some_nonexistent_type`
  - `store::tests::read_events_returns_all_events`
  - `store::tests::read_events_empty_stream_returns_empty_vec`
  - `store::tests::read_events_nonexistent_stream_returns_not_found`

## Concerns / Blockers
- The `#[allow(dead_code)]` on `StreamLayout::list_aggregate_types` in `src/storage.rs` could be removed now that it has a consumer, but that file is outside this ticket's scope. Minor cleanup for a future pass.
- None otherwise.
