# Code Review: Ticket 1 -- Add `source_device` field and `with_source_device` builder to `CommandContext`

**Ticket:** 1 -- Add `source_device` field and `with_source_device` builder to `CommandContext`
**Impl Report:** docs/prd/006-source-device-stamping-reports/ticket-01-impl.md
**Date:** 2026-02-22 13:00
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `CommandContext` has `pub source_device: Option<String>` with `#[serde(skip_serializing_if = "Option::is_none")]` | Met | Field at line 50 of `src/command.rs`. Also has `default` attribute (redundant for `Option<T>` but harmless). |
| 2 | `CommandContext::default()` produces `source_device == None` | Met | Verified by `default_context_has_no_fields_set` test (line 298-304). `Default` is derived and `Option<String>` defaults to `None`. |
| 3 | `with_source_device("device-abc")` sets `source_device == Some("device-abc".to_string())` | Met | Verified by `builder_sets_source_device` test (lines 340-343). |
| 4 | `with_source_device` accepts both `&str` and `String` (via `impl Into<String>`) | Met | Method signature at line 114: `pub fn with_source_device(mut self, device_id: impl Into<String>) -> Self`. Tested with `&str` in `builder_sets_source_device` and `String::from()` in `builder_accepts_string_owned` (line 352). |
| 5 | `source_device = None` serializes with no `"source_device"` key | Met | Verified by `source_device_none_omitted_from_json` test (lines 396-404). Uses `assert!(!json.contains("source_device"))`. |
| 6 | Deserializing JSON without `"source_device"` key produces `source_device == None` | Met | Verified by `deserialize_legacy_json_without_source_device` test (lines 406-415). Hard-coded legacy JSON string without the key. |
| 7 | `cargo test` passes with no new failures | Met | 90 unit tests + 7 doc-tests pass (verified by reviewer). |

## Issues Found

### Critical (must fix before merge)
- None.

### Major (should fix, risk of downstream problems)
- None.

### Minor (nice to fix, not blocking)
- **Redundant `default` serde attribute** (`src/command.rs` line 49): The `default` attribute on `#[serde(skip_serializing_if = "Option::is_none", default)]` is redundant for `Option<T>` fields -- serde already treats missing `Option` fields as `None`. The implementer documents this as intentional belt-and-suspenders in the impl report, which is a reasonable justification. Not blocking.
- **PRD doc status change** (`docs/prd/006-source-device-stamping.md`): The diff shows a one-line status change from "DRAFT" to "TICKETS READY". This is outside the ticket's stated scope (`src/command.rs` only). However, it is a non-functional documentation change with zero risk, likely made by the orchestrator tooling. Noting for completeness but not flagging as a scope violation.
- **Asymmetric serde annotations**: The existing `actor`, `correlation_id`, and `metadata` fields have no `skip_serializing_if` or `default` attributes, so they serialize as `"field": null` when `None`. The new `source_device` field skips the key entirely when `None`. This asymmetry is intentional and correct -- `source_device` was added after the initial release, so it must be absent (not null) in serialized output for backward compatibility with older records. The existing fields were always present from v0.1.0. No action needed, but worth documenting if consistency is ever revisited.

## Suggestions (non-blocking)
- The `CommandContext` doc example (lines 23-34) could optionally include `with_source_device` to showcase the full builder API, but this is purely a style preference and not required by any AC.

## Scope Check
- Files within scope: YES -- `src/command.rs` is the only code file modified.
- Scope creep detected: MINOR -- `docs/prd/006-source-device-stamping.md` had a one-line status update (DRAFT -> TICKETS READY). This is cosmetic PRD metadata, not functional code, and carries zero risk.
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- The new field defaults to `None`, is skipped when `None` in serialization, and is ignored when absent in deserialization. All 90 existing tests pass without modification beyond additive assertions. The struct's `Default` derive is unaffected (new `Option` field defaults to `None` automatically).
- Security concerns: NONE
- Performance concerns: NONE
