# Tickets for PRD 006: Source Device Stamping

**Source PRD:** docs/prd/006-source-device-stamping.md
**Created:** 2026-02-22
**Total Tickets:** 3
**Estimated Total Complexity:** 5 (S=1 + M=2 + M=2)

---

### Ticket 1: Add `source_device` field and `with_source_device` builder to `CommandContext`

**Description:**
Add `source_device: Option<String>` to the `CommandContext` struct in `src/command.rs` with
`#[serde(skip_serializing_if = "Option::is_none")]`, and add the `with_source_device` builder
method following the identical pattern of `with_actor` and `with_correlation_id`. Update all
existing unit tests in `src/command.rs` that assert specific field counts or serde output to
account for the new field (most will pass without change; the serde round-trip test needs the
new field included in its assertions).

**Scope:**
- Modify: `src/command.rs`

**Acceptance Criteria:**
- [ ] `CommandContext` has a `pub source_device: Option<String>` field annotated with
  `#[serde(skip_serializing_if = "Option::is_none")]`.
- [ ] `CommandContext::default()` produces `source_device == None`.
- [ ] `CommandContext::default().with_source_device("device-abc")` returns a context where
  `source_device == Some("device-abc".to_string())`.
- [ ] `with_source_device` accepts both `&str` and `String` (via `impl Into<String>`),
  consistent with `with_actor` and `with_correlation_id`.
- [ ] A `CommandContext` with `source_device = None` serializes to JSON with no
  `"source_device"` key present.
- [ ] A `CommandContext` deserialized from a JSON object that has no `"source_device"` key
  (simulating a prior-version record) produces `source_device == None`.
- [ ] `cargo test` passes with no new failures.

**Dependencies:** None
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 2, AC 3, AC 8

---

### Ticket 2: Stamp `source_device` into `event.meta` in `to_eventfold_event`

**Description:**
In `src/aggregate.rs`, inside `to_eventfold_event`, insert a `source_device` key into
`meta_map` immediately after the `correlation_id` insertion block, following the same
`if let Some(ref ...) { meta_map.insert(...) }` pattern. Add focused unit tests in the
`#[cfg(test)] mod tests` block covering: source_device alone, source_device + correlation_id
together, source_device + existing metadata object merged, and the absence-of-meta guard
(all None context leaves `event.meta == None`).

**Scope:**
- Modify: `src/aggregate.rs`

**Acceptance Criteria:**
- [ ] When `to_eventfold_event` is called with `source_device = Some("device-xyz")`, the
  resulting `eventfold::Event` has `meta["source_device"] == "device-xyz"`.
- [ ] When `to_eventfold_event` is called with `source_device = None`, `correlation_id = None`,
  and `metadata = None`, the resulting `eventfold::Event` has `meta == None` (no empty object
  attached).
- [ ] When `to_eventfold_event` is called with both `source_device` and `correlation_id` set,
  `event.meta` contains both `"source_device"` and `"correlation_id"` keys in the same JSON
  object.
- [ ] When `to_eventfold_event` is called with `source_device` set alongside a `ctx.metadata`
  that is already a JSON object (e.g. `{"foo": "bar"}`), the resulting `event.meta` contains
  `"source_device"`, `"foo"`, and all other pre-existing keys — no keys are dropped.
- [ ] `cargo test` passes with no new failures.

**Dependencies:** Ticket 1
**Complexity:** M
**Maps to PRD AC:** AC 4, AC 5, AC 6, AC 7

---

### Ticket 3: Verification and Integration Check

**Description:**
Run the full PRD acceptance criteria checklist end-to-end. Verify both modified files compile
cleanly, all tests pass, clippy produces no warnings, and formatting is clean. Confirm
backward-compatibility by deserializing a hard-coded JSON string that lacks `"source_device"`
into `CommandContext` and asserting the field is `None`.

**Acceptance Criteria:**
- [ ] All PRD acceptance criteria (AC 1-10) pass.
- [ ] `cargo test` exits 0 with no new failures.
- [ ] `cargo clippy -- -D warnings` produces zero warnings.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo build` produces zero compiler warnings.

**Dependencies:** Ticket 1, Ticket 2
**Complexity:** S
**Maps to PRD AC:** AC 9, AC 10

---

## AC Coverage Matrix

| PRD AC # | Description                                                                                                                         | Covered By Ticket(s) | Status  |
|----------|-------------------------------------------------------------------------------------------------------------------------------------|----------------------|---------|
| 1        | `CommandContext::default()` has `source_device == None`; serializes with no `source_device` key                                    | Ticket 1             | Covered |
| 2        | `with_source_device("device-abc")` sets `source_device == Some("device-abc".to_string())`                                          | Ticket 1             | Covered |
| 3        | `with_source_device` accepts both `&str` and `String` via `impl Into<String>`                                                       | Ticket 1             | Covered |
| 4        | `source_device = Some("device-xyz")` in context produces `meta["source_device"] == "device-xyz"` on the event                      | Ticket 2             | Covered |
| 5        | All-None context (source_device, correlation_id, metadata all None) produces `event.meta == None`                                   | Ticket 2             | Covered |
| 6        | Both `source_device` and `correlation_id` set produces event.meta with both keys                                                    | Ticket 2             | Covered |
| 7        | `source_device` merges into pre-existing `ctx.metadata` JSON object without dropping any keys                                       | Ticket 2             | Covered |
| 8        | Old `CommandContext` JSON without `"source_device"` key deserializes successfully with `source_device == None`                      | Ticket 1             | Covered |
| 9        | `cargo test` passes with no new failures                                                                                            | Ticket 3             | Covered |
| 10       | `cargo clippy -- -D warnings` produces no warnings on modified files                                                                | Ticket 3             | Covered |
