# Code Review: Ticket 2 -- Stamp `source_device` into `event.meta` in `to_eventfold_event`

**Ticket:** 2 -- Stamp `source_device` into `event.meta` in `to_eventfold_event`
**Impl Report:** docs/prd/006-source-device-stamping-reports/ticket-02-impl.md
**Date:** 2026-02-22 17:00
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `source_device = Some("device-xyz")` produces `meta["source_device"] == "device-xyz"` | Met | `context_propagates_source_device` test (line 420-426) constructs context with `with_source_device("device-xyz")`, calls `to_eventfold_event`, and asserts `meta["source_device"] == "device-xyz"`. Production code at lines 159-164 inserts the key correctly. |
| 2 | All-None context produces `event.meta == None` | Met | `all_none_context_produces_no_meta` test (lines 454-468) constructs default context, asserts all three fields are `None`, then verifies `event.meta.is_none()`. Guard at line 166 (`if !meta_map.is_empty()`) ensures no empty object is attached. |
| 3 | Both `source_device` and `correlation_id` set produces both keys in `event.meta` | Met | `context_propagates_source_device_and_correlation_id` test (lines 429-438) sets both fields, asserts both keys present in the same `meta` object. |
| 4 | `source_device` merges with existing `ctx.metadata` JSON object without dropping keys | Met | `context_merges_source_device_with_existing_metadata` test (lines 441-451) sets metadata to `{"foo": "bar", "level": 3}` plus `source_device("device-xyz")`, asserts all three keys present. The merge works because `meta_map` starts from `ctx.metadata` (line 148) and `source_device` is inserted additively (line 160). |
| 5 | `cargo test` passes | Met | 94 unit tests + 7 doc-tests all pass (verified by reviewer: `cargo test` output shows 0 failures). |

## Issues Found

### Critical (must fix before merge)
- None.

### Major (should fix, risk of downstream problems)
- None.

### Minor (nice to fix, not blocking)
- None.

## Suggestions (non-blocking)
- The `source_device` insertion block (lines 159-164) is positioned after `correlation_id` (lines 152-157) as specified by the ticket. The pattern is identical: `if let Some(ref ...) { meta_map.insert(...) }`. Clean and consistent.
- The `all_none_context_produces_no_meta` test includes explicit precondition assertions (`ctx.source_device.is_none()`, etc.) before the main assertion. This is good defensive testing practice -- if `CommandContext::default()` ever changes, the test will fail at the precondition rather than producing a confusing failure at the meta assertion.

## Scope Check
- Files within scope: YES -- only `src/aggregate.rs` was modified.
- Scope creep detected: NO -- changes are limited to the `source_device` insertion in `to_eventfold_event` (6 lines of production code) and 4 focused tests (51 lines of test code).
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- The new insertion block is additive. It only fires when `ctx.source_device` is `Some(...)`, which requires an explicit `with_source_device()` call. Existing code paths that do not set `source_device` are completely unaffected. All 94 existing tests pass without modification.
- Security concerns: NONE -- `source_device` is caller-supplied metadata with no special privileges.
- Performance concerns: NONE -- one additional `Option` check and potential `String` clone per event, negligible cost.
