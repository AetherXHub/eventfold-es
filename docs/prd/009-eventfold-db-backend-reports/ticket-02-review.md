# Code Review: Ticket 2 -- src/event.rs: StoredEvent, EventMetadata, encode/decode helpers, stream_uuid

**Ticket:** 2 -- src/event.rs: StoredEvent, EventMetadata, encode/decode helpers, stream_uuid
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-02-impl.md
**Date:** 2026-02-27 16:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `StoredEvent` struct with 10 fields; derives `Debug, Clone` | Met | `src/event.rs` lines 218-239. All 10 fields present with correct types: `event_id: Uuid`, `stream_id: Uuid`, `aggregate_type: String`, `instance_id: String`, `stream_version: u64`, `global_position: u64`, `event_type: String`, `payload: serde_json::Value`, `metadata: serde_json::Value`, `recorded_at: u64`. Derives `Debug, Clone`. |
| 2 | `EventMetadata` with 5 fields; `Serialize, Deserialize`; `skip_serializing_if` on optionals | Met | `src/event.rs` lines 56-71. All 5 fields present. Derives `Debug, Clone, Serialize, Deserialize`. `#[serde(skip_serializing_if = "Option::is_none")]` on `actor`, `correlation_id`, `source_device`. |
| 3 | `STREAM_NAMESPACE: Uuid` matches PRD bytes exactly | Met | `src/event.rs` lines 15-17. Bytes are `[0x9a, 0x1e, 0x7c, 0x3b, 0x4d, 0x2f, 0x4a, 0x8e, 0xb5, 0x6c, 0x1f, 0x3d, 0x7e, 0x9a, 0x0b, 0xc4]`, matching PRD and ticket AC exactly. `const` (not `pub`) -- correct as implementation detail. |
| 4 | `stream_uuid` uses `Uuid::new_v5` with `"{aggregate_type}/{instance_id}"` | Met | `src/event.rs` lines 42-45. Uses `format!("{aggregate_type}/{instance_id}")` and `Uuid::new_v5(&STREAM_NAMESPACE, name.as_bytes())`. Doc comment with working doctest. |
| 5 | `encode_domain_event` serializes adjacently-tagged event, extracts type/data, populates metadata, generates UUID v4 | Met | `src/event.rs` lines 110-146. Serializes via `serde_json::to_value`, extracts `"type"` and `"data"` fields, builds `EventMetadata` from context, generates `Uuid::new_v4()`. Returns `serde_json::Result<ProposedEventData>`. |
| 6 | `decode_stored_event` parses metadata JSON, returns `None` on missing/invalid metadata | Met | `src/event.rs` lines 169-210. Parses UTF-8, JSON, extracts `aggregate_type`/`instance_id` with empty-string filter, parses UUIDs, handles empty payload. Returns `Option<StoredEvent>`. |
| 7 | Test: `stream_uuid` determinism | Met | `stream_uuid_is_deterministic` at line 246. |
| 8 | Test: `stream_uuid` uniqueness by instance_id and aggregate_type | Met | `stream_uuid_differs_by_instance_id` at line 253, `stream_uuid_differs_by_aggregate_type` at line 318. |
| 9 | Test: `encode_domain_event` with `Incremented` produces correct type, payload, metadata | Met | `encode_incremented_produces_correct_event_type_and_metadata` at line 333, `encode_fieldless_variant_produces_empty_object_or_null_payload` at line 351. Also `encode_variant_with_data_includes_payload` at line 367 for the `Added` variant. UUID v4 version verified. |
| 10 | Test: `encode_domain_event` with full `CommandContext` populates all optional metadata | Met | `encode_populates_all_optional_metadata_fields` at line 382. Tests `with_actor("u1")`, `with_correlation_id("c1")`, `with_source_device("d1")`. |
| 11 | Test: `decode_stored_event` with well-formed metadata returns `Some` | Met | `decode_well_formed_recorded_event_returns_some` at line 428. Verifies all fields including `aggregate_type`, `instance_id`, `stream_version`, `global_position`, `event_type`, `recorded_at`, `stream_id`. |
| 12 | Test: `decode_stored_event` with `b"{}"` returns `None` | Met | `decode_empty_json_object_returns_none` at line 449. |
| 13 | Test: `decode_stored_event` with invalid UTF-8/JSON returns `None` | Met | `decode_invalid_json_returns_none` at line 459. Uses `vec![0xFF, 0xFE]` for invalid UTF-8. |
| 14 | Quality gates pass | Met | Verified: `cargo build` (zero warnings), `cargo clippy -- -D warnings` (zero warnings), `cargo fmt --check` (no diffs), `cargo test` (52 unit + 2 doctests, all passing). |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **Out-of-scope modifications to `aggregate.rs` and `command.rs`.** The ticket scope lists `src/event.rs` (create) and `src/lib.rs` (modify). The implementer also modified `aggregate.rs` (removed `reducer`, `reduce`, `to_eventfold_event`, and 15 bridge tests; removed `use crate::command::CommandContext` and `use uuid::Uuid` imports) and `command.rs` (removed `CommandBus`, `CommandRoute`, `TypedCommandRoute`, 2 async tests; removed 6 unused imports; updated doc comments removing `eventfold::Event` references). These changes are strictly subtractive (removing code that cannot compile after the `eventfold` crate was removed in Ticket 1) and were necessary to make the modules compile after uncommenting `mod aggregate` and `mod command` in `lib.rs`. The ticket's IMPORTANT NOTES stated these modules needed to be uncommented. While technically a scope violation, the changes are forced by the build constraint and align with the PRD's stated removals. Ticket 7 (the dedicated aggregate cleanup ticket) now has no remaining work. Flagging as Minor because the deviation is mechanically necessary, not discretionary.

2. **Removed 17 tests from aggregate.rs and command.rs.** 15 bridge tests (`reducer_roundtrip_*`, `context_propagates_*`, `to_eventfold_event_*`, etc.) and 2 `CommandBus` async tests were deleted. The bridge tests validated code that no longer exists (the `eventfold` bridge functions), and the CommandBus tests referenced `AggregateStore` which is still commented out. The equivalent encode/decode functionality is now tested in `event.rs`. The CommandBus will be re-added in Ticket 11 when `AggregateStore` is rewritten.

3. **`DispatchError` doc comment references `CommandBus`** in `error.rs` line 76 (`/// Produced by the dispatch layer ([`CommandBus`](crate::CommandBus) or...`). Since `CommandBus` was removed in this ticket's `command.rs` changes, this link is now broken. The `error.rs` file was not claimed as modified by this ticket, so this is a latent issue that Ticket 3 (or a later ticket re-adding CommandBus) should address.

## Suggestions (non-blocking)

1. **`decode_stored_event` empty-string filter.** Lines 178 and 184 use `.filter(|s| !s.is_empty())?` to reject empty `aggregate_type` and `instance_id` strings. This is a good defensive measure beyond what the AC strictly requires (AC says "absent" not "empty"). Well done.

2. **`ProposedEventData` derives.** The struct derives `Debug, Clone` but not `PartialEq`. Adding `PartialEq` would make testing easier in downstream tickets (Ticket 4 roundtrip tests). Not required by the AC.

3. **Test coverage.** 15 tests is thorough. The `decode_missing_instance_id_returns_none` test (line 480) is a nice bonus beyond the AC requirements, covering the case where `aggregate_type` is present but `instance_id` is missing.

4. **`encode_domain_event` `.expect()` calls.** Lines 122 and 125 use `.expect()` on post-serialization invariants. These are legitimate invariant violations per the project's convention (CLAUDE.md: "use `.expect()` only for invariant violations with a descriptive message"). The descriptive messages clearly explain the invariant. The only risk is if a user implements `DomainEvent` without the adjacently-tagged serde attribute, which would violate the `Aggregate` trait contract.

## Scope Check

- Files within scope: YES -- `src/event.rs` created, `src/lib.rs` modified.
- Out-of-scope files touched: `src/aggregate.rs`, `src/command.rs`. Both modifications are strictly subtractive removals of code that cannot compile without the removed `eventfold` dependency. Necessary to satisfy the build quality gate (AC 14). See Minor issue #1.
- Scope creep detected: NO -- The out-of-scope changes are mechanically forced by the build constraint, not feature additions or discretionary cleanups. No new abstractions or patterns were introduced beyond what the ticket requires.
- Unauthorized dependencies added: NO

## Risk Assessment

- Regression risk: LOW -- All removed code depended on the `eventfold` crate which was already removed in Ticket 1. The `aggregate.rs` and `command.rs` modules retain their core types (`Aggregate` trait, `CommandContext`, `CommandEnvelope`) and all non-bridge tests. The new `event.rs` module is additive with no callers yet (downstream tickets will wire it in).
- Security concerns: NONE
- Performance concerns: NONE -- `encode_domain_event` allocates a `serde_json::Value` intermediate and a `String` for the format string, which is appropriate for an event serialization path (not a hot loop). `decode_stored_event` similarly allocates minimally.
