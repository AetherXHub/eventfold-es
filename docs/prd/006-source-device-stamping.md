# PRD 005: Source Device Stamping in CommandContext

**Status:** TICKETS READY
**Created:** 2026-02-22
**Author:** PRD Writer Agent
**GitHub Issue:** #3

---

## Problem Statement

Each client install in a multi-device deployment carries a unique device ID. When
events are synced to peer clients, the receiving client cannot determine which device
originated a given event — making it impossible to filter out self-originated events
in process managers or to implement per-device guards. A `source_device` field on
`CommandContext` that gets stamped into `event.meta` during `to_eventfold_event()`
closes this gap with zero breaking changes, since `event.meta` is already
`Option<Value>` and serialization of the field is skipped when absent.

## Goals

- `CommandContext` carries an optional `source_device` field that callers can set
  per-command without altering their existing code paths.
- `source_device` is written into `event.meta` under the key `"source_device"` for
  every event produced by that command, making it available to projections, process
  managers, and downstream sync consumers reading raw JSONL.
- The change is fully backward-compatible: existing serialized events and existing call
  sites that do not set `source_device` are unaffected.

## Non-Goals

- Generating or managing device IDs. Callers supply the device ID string; this crate
  does not mint, persist, or validate it.
- Enforcing uniqueness or format of device ID strings. Any `String` is accepted.
- Cross-process or network device registry. This is a pure metadata-stamping feature.
- Automatic injection of a process-wide device ID (e.g. from an environment variable
  or config file). Callers pass `source_device` explicitly via the builder.
- Changes to `CommandEnvelope`, `CommandBus`, or any process manager or projection
  trait.

## User Stories

- As a process manager author, I want to read `event.meta["source_device"]` to skip
  commands triggered by events the local device itself originated, so that I avoid
  reprocessing my own events on sync.
- As a sync consumer, I want every event's JSONL record to carry `source_device` in
  `meta` so that I can route or filter events by originating device without inspecting
  application-level payload fields.
- As a library user, I want to set `source_device` on `CommandContext` with the same
  fluent builder pattern as `actor` and `correlation_id`, so that the API is
  consistent and I do not need to manually merge JSON objects.

## Technical Approach

All changes are confined to a single file: `src/command.rs`.

### `CommandContext` struct

Add one field:

```rust
/// The device ID of the client that issued the command.
/// Stamped into `event.meta["source_device"]` by `to_eventfold_event`.
pub source_device: Option<String>,
```

The field serializes and deserializes with `serde` using
`#[serde(skip_serializing_if = "Option::is_none")]` so that existing JSONL records
that lack the field still deserialize cleanly, and newly written records with
`source_device = None` do not grow in size.

### Builder method

Add a `with_source_device` builder method following the identical pattern of
`with_actor` and `with_correlation_id`:

```rust
pub fn with_source_device(mut self, device_id: impl Into<String>) -> Self {
    self.source_device = Some(device_id.into());
    self
}
```

### `to_eventfold_event` in `src/aggregate.rs`

In the `meta_map` construction block (after the `correlation_id` insertion), add:

```rust
if let Some(ref device_id) = ctx.source_device {
    meta_map.insert(
        "source_device".to_string(),
        serde_json::Value::String(device_id.clone()),
    );
}
```

This integrates with the existing merge logic: if `ctx.metadata` is already an
`Object`, `source_device` is merged in alongside `correlation_id`; if metadata is
absent or non-Object, `meta_map` starts empty and `source_device` is the only key
(subject to the existing `if !meta_map.is_empty()` guard that attaches meta only when
needed).

### File change summary

| File | Change |
|---|---|
| `src/command.rs` | Add `source_device: Option<String>` field + `with_source_device` builder |
| `src/aggregate.rs` | Insert `source_device` into `meta_map` inside `to_eventfold_event` |

No other files require modification. No new dependencies are introduced.

## Acceptance Criteria

1. `CommandContext::default()` has `source_device == None`; a `CommandContext`
   constructed without calling `with_source_device` serializes to JSON with no
   `source_device` key present.
2. `CommandContext::default().with_source_device("device-abc")` returns a context
   where `ctx.source_device == Some("device-abc".to_string())`.
3. `with_source_device` accepts both `&str` and `String` values (via `impl Into<String>`),
   consistent with `with_actor` and `with_correlation_id`.
4. When `to_eventfold_event` is called with a context where `source_device` is
   `Some("device-xyz")`, the resulting `eventfold::Event` has
   `meta["source_device"] == "device-xyz"`.
5. When `to_eventfold_event` is called with a context where `source_device` is `None`
   and `correlation_id` is also `None` and `metadata` is `None`, the resulting
   `eventfold::Event` has `meta == None` (no empty meta object is attached).
6. When `to_eventfold_event` is called with both `source_device` and `correlation_id`
   set, `event.meta` contains both `"source_device"` and `"correlation_id"` keys in
   the same JSON object.
7. When `to_eventfold_event` is called with `source_device` set alongside a
   `ctx.metadata` that is already a JSON object (e.g. `{"foo": "bar"}`), the resulting
   `event.meta` contains `"source_device"`, `"foo"`, and any other existing keys —
   no keys are dropped.
8. An existing `CommandContext` serialized to JSON without a `source_device` key (as
   produced by prior versions of the library) deserializes successfully into the new
   `CommandContext` struct with `source_device == None`.
9. `cargo test` passes with no new failures.
10. `cargo clippy -- -D warnings` produces no warnings on the modified files.

## Open Questions

- Should `source_device` be surfaced as a first-class field in `CommandEnvelope` (for
  process-manager-to-process-manager dispatch) or is propagating it through
  `ctx.metadata` sufficient? Current decision: `CommandEnvelope` already carries a
  full `CommandContext`; if the dispatching process manager copies `source_device` into
  the outgoing `CommandContext`, it flows naturally. No changes to `CommandEnvelope`
  are required by this PRD.

## Dependencies

- `eventfold` v0.2.0 — `eventfold::Event::meta` field is `Option<Value>` (confirmed in
  `src/aggregate.rs` implementation).
- No new crate dependencies.
