# PRD 001: Auto-Generate Event IDs (UUID v4) in `to_eventfold_event`

**Status:** DRAFT
**Created:** 2026-02-22
**Author:** PRD Writer Agent

---

## Problem Statement

Events written through `to_eventfold_event` currently have no `id` field set, so they are stored with `id: None` in the JSONL log. Without a globally unique identifier on each event, a relay server that syncs events between clients cannot deduplicate events that arrive through multiple paths. Deduplication requires every event to carry a stable, unique identity at the moment it is created.

## Goals

- Every event emitted via `to_eventfold_event` carries a UUID v4 string in `eventfold::Event::id`.
- The change is backward-compatible: existing JSONL logs (where `id` is absent) continue to replay correctly because `Event::id` is `Option<String>` with `skip_serializing_if`.
- Callers that already supply an `id` on the returned `Event` are not broken.

## Non-Goals

- This PRD does not define how a relay server uses event IDs for deduplication — that is a separate concern.
- This PRD does not add any deduplication logic inside `eventfold-es`.
- This PRD does not expose a mechanism for callers to supply a custom event ID; the ID is always generated automatically. If an override mechanism is needed in the future it is a separate PRD.
- This PRD does not change the storage format, JSONL schema, or the `eventfold` crate itself.
- This PRD does not affect the `reduce` function or the projection/process-manager read paths.

## User Stories

- As a library consumer syncing events to a relay server, I want each emitted event to have a unique `id`, so that the relay can detect and discard duplicate deliveries.
- As a library consumer replaying an existing JSONL log that pre-dates this change, I want events without an `id` to still apply correctly, so that I do not need to migrate historical data.
- As a contributor reading the code, I want the ID generation to be visible at the single call site in `to_eventfold_event`, so that the source of event IDs is unambiguous.

## Technical Approach

### Affected files

| File | Change |
|------|--------|
| `Cargo.toml` | Add `uuid = { version = "1", features = ["v4"] }` to `[dependencies]` |
| `src/aggregate.rs` | Add `use uuid::Uuid;`, call `event.with_id(Uuid::new_v4().to_string())` immediately after `eventfold::Event::new(...)` in `to_eventfold_event` |

### Detail

`to_eventfold_event` in `src/aggregate.rs` (line 118) constructs an `eventfold::Event` via `eventfold::Event::new(event_type, data)` and then chains builder calls (`with_actor`, `with_meta`). A single additional builder call `event.with_id(Uuid::new_v4().to_string())` should be added immediately after `Event::new`, before the actor and meta assignments.

The `eventfold` crate's `Event` struct already has `id: Option<String>` with `#[serde(skip_serializing_if = "Option::is_none")]`. The builder method is `with_id(impl Into<String>) -> Event`. No changes to the `eventfold` crate are required.

`uuid` version `1` with feature `v4` is the standard choice. `Uuid::new_v4()` is infallible and requires no additional error handling. The UUID is formatted as a lowercase hyphenated string via `Uuid::to_string()` (e.g. `"550e8400-e29b-41d4-a716-446655440000"`).

No changes to `reduce`, `reducer`, any projection or process-manager code, or any public API signatures are required.

## Acceptance Criteria

1. After the change, calling `to_eventfold_event::<A>(event, &ctx)` returns an `eventfold::Event` whose `id` field is `Some(s)` where `s` is a valid UUID v4 string matching the regex `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`.
2. Two successive calls to `to_eventfold_event` with identical arguments produce events whose `id` values are different from each other.
3. `cargo test` passes with no failures (all existing tests continue to pass).
4. `cargo build` produces no compiler warnings.
5. `cargo clippy -- -D warnings` produces no warnings or errors.
6. `cargo fmt --check` reports no formatting violations.
7. `Cargo.toml` contains `uuid` in `[dependencies]` with the `v4` feature enabled and no other uuid features added beyond what is necessary.
8. The `eventfold::Event::id` field is absent from JSONL output for events that were written before this change (i.e. events loaded from a pre-existing log where `id` was `None` must still be applied by `reduce` without error).

## Open Questions

- Does `eventfold::Event` expose `with_id` as a builder method, or must `id` be set by direct field assignment? The project memory confirms `id: Option<String>` exists on `Event`; the builder method name should be verified against the `eventfold` 0.2.0 source before implementation. If `with_id` does not exist, direct field assignment (`event.id = Some(...)`) is the fallback.

## Dependencies

- `eventfold` 0.2.0 — `Event::id: Option<String>` with `skip_serializing_if` (already a dependency; no version change required).
- `uuid` 1.x — new dependency to add to `Cargo.toml`.
