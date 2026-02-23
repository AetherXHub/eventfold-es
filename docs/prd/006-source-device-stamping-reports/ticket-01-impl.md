# Implementation Report: Ticket 1 -- Add `source_device` field and `with_source_device` builder to `CommandContext`

**Ticket:** 1 - Add `source_device` field and `with_source_device` builder to `CommandContext`
**Date:** 2026-02-22 12:00
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/command.rs` - Added `source_device: Option<String>` field to `CommandContext` with `#[serde(skip_serializing_if = "Option::is_none", default)]`, added `with_source_device` builder method, updated existing tests, and added new tests for the field.

## Implementation Notes
- The `source_device` field uses `#[serde(skip_serializing_if = "Option::is_none", default)]`. The `default` attribute is added alongside `skip_serializing_if` to explicitly guarantee that JSON objects missing the `source_device` key deserialize with `source_device = None`. While serde handles `Option<T>` fields this way by default, the explicit `default` attribute makes the backward-compatibility intent clear and guards against future serde behavior changes.
- The `with_source_device` builder method follows the identical pattern of `with_actor` and `with_correlation_id`: takes `impl Into<String>`, sets the field to `Some(value.into())`, returns `self`.
- Existing tests (`default_context_has_no_fields_set`, `builder_chains_all_fields`, `builder_accepts_string_owned`, `command_context_serde_roundtrip`) were updated to include `source_device` assertions.
- Two new tests added: `source_device_none_omitted_from_json` (verifies skip_serializing_if) and `deserialize_legacy_json_without_source_device` (verifies backward compatibility).
- One new test added: `builder_sets_source_device` (verifies the builder in isolation).

## Acceptance Criteria
- [x] AC 1: `CommandContext` has a `pub source_device: Option<String>` field annotated with `#[serde(skip_serializing_if = "Option::is_none")]` - Field added at line 50 with both `skip_serializing_if` and `default` annotations.
- [x] AC 2: `CommandContext::default()` produces `source_device == None` - Verified by `default_context_has_no_fields_set` test (line 303).
- [x] AC 3: `CommandContext::default().with_source_device("device-abc")` returns a context where `source_device == Some("device-abc".to_string())` - Verified by `builder_sets_source_device` test (line 340-343).
- [x] AC 4: `with_source_device` accepts both `&str` and `String` (via `impl Into<String>`) - Method signature uses `impl Into<String>`; verified by `builder_accepts_string_owned` test using `String::from()` and `builder_sets_source_device` using `&str`.
- [x] AC 5: A `CommandContext` with `source_device = None` serializes to JSON with no `"source_device"` key present - Verified by `source_device_none_omitted_from_json` test (line 396-404).
- [x] AC 6: A `CommandContext` deserialized from a JSON object that has no `"source_device"` key produces `source_device == None` - Verified by `deserialize_legacy_json_without_source_device` test (line 406-415).
- [x] AC 7: `cargo test` passes with no new failures - All 90 unit tests + 7 doc-tests pass.

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (90 unit tests, 7 doc-tests, 0 failures)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt --check` -- no changes needed)
- New tests added:
  - `command::tests::builder_sets_source_device` in `src/command.rs`
  - `command::tests::source_device_none_omitted_from_json` in `src/command.rs`
  - `command::tests::deserialize_legacy_json_without_source_device` in `src/command.rs`

## Concerns / Blockers
- None. The implementation is straightforward and stays within the single-file scope (`src/command.rs`).
- Note for downstream ticket (Ticket 2, likely): The `to_eventfold_event` function in `src/aggregate.rs` will need to stamp `source_device` into `event.meta` -- this is the PRD's AC 4-7 which are not in scope for this ticket but are referenced in the PRD.
