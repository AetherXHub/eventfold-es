# Implementation Report: Ticket 2 -- src/event.rs: StoredEvent, EventMetadata, encode/decode helpers, stream_uuid

**Ticket:** 2 - src/event.rs: StoredEvent, EventMetadata, encode/decode helpers, stream_uuid
**Date:** 2026-02-27 12:00
**Status:** COMPLETE

---

## Files Changed

### Created
- `src/event.rs` - New module containing `StoredEvent`, `EventMetadata`, `ProposedEventData`, `STREAM_NAMESPACE`, `stream_uuid()`, `encode_domain_event()`, `decode_stored_event()`, and 15 unit tests.

### Modified
- `src/lib.rs` - Uncommented `mod aggregate`, `mod command`, `mod error`; added `mod event` and `pub use` for all new public items (`StoredEvent`, `EventMetadata`, `ProposedEventData`, `stream_uuid`, `encode_domain_event`, `decode_stored_event`); added re-exports for `Aggregate`, `CommandContext`, `CommandEnvelope`, `DispatchError`, `ExecuteError`, `StateError`.
- `src/aggregate.rs` - Removed `eventfold`-dependent functions (`reduce()`, `reducer()`, `to_eventfold_event()`) and their 15 tests, since the `eventfold` crate dependency was removed in Ticket 1. Removed unused `use uuid::Uuid` and `use crate::command::CommandContext` imports. Updated module doc comment.
- `src/command.rs` - Removed `CommandBus`, `CommandRoute` trait, `TypedCommandRoute` struct, and their tests, since they depended on `crate::store::AggregateStore` which is not yet available (store module is still commented out). Removed unused imports (`std::any`, `std::collections::HashMap`, `std::future::Future`, `std::pin::Pin`, `crate::aggregate::Aggregate`, `crate::error::DispatchError`, `crate::store::AggregateStore`). Updated doc comments to remove `eventfold::Event` references.

## Implementation Notes
- The ticket scope listed only `src/event.rs` (create) and `src/lib.rs` (modify), but the IMPORTANT NOTES section stated that `mod aggregate`, `mod command`, and `mod error` needed to be uncommented. This required modifying `aggregate.rs` and `command.rs` to remove code that referenced the now-removed `eventfold` crate and the not-yet-available `store` module. These changes are strictly subtractive (removing code that cannot compile) and align with the PRD's stated removals.
- `encode_domain_event` takes explicit `aggregate_type` and `instance_id` parameters rather than using `A::AGGREGATE_TYPE` directly. This matches the ticket's function signature and gives callers flexibility.
- `decode_stored_event` returns `None` (not `Err`) for unparseable events, matching the PRD's "silently skipped" design for non-eventfold-es events.
- `STREAM_NAMESPACE` is a module-level `const` (not `pub`) since it is an implementation detail used only by `stream_uuid()`. The exact bytes match the PRD specification.
- The `ProposedEventData` struct is a Rust-native intermediate, distinct from the proto `ProposedEvent`. This allows typed inspection before gRPC serialization.

## Acceptance Criteria
- [x] AC 1: `StoredEvent` struct has all specified fields (`event_id`, `stream_id`, `aggregate_type`, `instance_id`, `stream_version`, `global_position`, `event_type`, `payload`, `metadata`, `recorded_at`); derives `Debug, Clone`
- [x] AC 2: `EventMetadata` struct has all specified fields with `Serialize, Deserialize` derives and `#[serde(skip_serializing_if = "Option::is_none")]` on optional fields
- [x] AC 3: `STREAM_NAMESPACE: Uuid` constant uses the exact bytes from the PRD
- [x] AC 4: `stream_uuid()` uses `Uuid::new_v5(&STREAM_NAMESPACE, ...)` with `"{aggregate_type}/{instance_id}"` format
- [x] AC 5: `encode_domain_event()` serializes adjacently-tagged domain event, extracts `type`/`data`, populates `EventMetadata`, generates UUID v4 `event_id`, returns `ProposedEventData`
- [x] AC 6: `decode_stored_event()` parses `RecordedEvent.metadata` as JSON `EventMetadata`, returns `None` on missing/unparseable fields, returns `Some(StoredEvent)` on success
- [x] AC 7: Test: `stream_uuid("counter", "c-1")` called twice returns same value (determinism) - `stream_uuid_is_deterministic`
- [x] AC 8: Test: `stream_uuid` produces different UUIDs for different inputs (uniqueness) - `stream_uuid_differs_by_instance_id`, `stream_uuid_differs_by_aggregate_type`
- [x] AC 9: Test: `encode_domain_event` with `Incremented` produces correct event_type, payload, metadata - `encode_incremented_produces_correct_event_type_and_metadata`, `encode_fieldless_variant_produces_empty_object_or_null_payload`
- [x] AC 10: Test: `encode_domain_event` with full `CommandContext` populates all optional metadata - `encode_populates_all_optional_metadata_fields`
- [x] AC 11: Test: `decode_stored_event` with well-formed metadata returns `Some` with matching fields - `decode_well_formed_recorded_event_returns_some`
- [x] AC 12: Test: `decode_stored_event` with `b"{}"` metadata returns `None` - `decode_empty_json_object_returns_none`
- [x] AC 13: Test: `decode_stored_event` with invalid UTF-8/JSON returns `None` - `decode_invalid_json_returns_none`
- [x] AC 14: Quality gates pass (build, lint, fmt, tests)

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (52 unit tests + 2 doctests, all passing)
- Build: PASS (`cargo build` -- zero warnings)
- Fmt: PASS (`cargo fmt --check` -- no diffs)
- New tests added: 15 tests in `src/event.rs::tests`:
  - `stream_uuid_is_deterministic`
  - `stream_uuid_differs_by_instance_id`
  - `stream_uuid_differs_by_aggregate_type`
  - `event_metadata_skips_none_fields_in_serialization`
  - `event_metadata_includes_present_fields`
  - `event_metadata_serde_roundtrip`
  - `encode_incremented_produces_correct_event_type_and_metadata`
  - `encode_fieldless_variant_produces_empty_object_or_null_payload`
  - `encode_variant_with_data_includes_payload`
  - `encode_populates_all_optional_metadata_fields`
  - `encode_omits_none_optional_metadata_fields`
  - `decode_well_formed_recorded_event_returns_some`
  - `decode_empty_json_object_returns_none`
  - `decode_invalid_json_returns_none`
  - `decode_missing_instance_id_returns_none`

## Concerns / Blockers
- **Out-of-scope file modifications:** `aggregate.rs` and `command.rs` were modified to remove code that referenced the removed `eventfold` crate and the not-yet-available `store` module. These changes were necessary to make the modules compile after uncommenting them in `lib.rs`, as directed by the ticket's IMPORTANT NOTES. The changes are strictly subtractive and align with the PRD's stated removals. Future tickets that rewrite `command.rs` (CommandBus) or `store.rs` will need to re-add `CommandBus` with the new `AggregateStore` dependency.
- **Removed tests:** 17 tests were removed from `aggregate.rs` (15 bridge tests for `reducer`/`to_eventfold_event` + 2 CommandBus tests from `command.rs`). These tests validated code that no longer exists. The equivalent encode/decode functionality is now tested in `event.rs`.
