# Tickets for PRD 005: Auto-Generate Event IDs (UUID v4) in `to_eventfold_event`

**Source PRD:** docs/prd/005-auto-generate-event-ids.md
**Created:** 2026-02-22
**Total Tickets:** 2
**Estimated Total Complexity:** 3 (S=1 + M=2)

---

### Ticket 1: Add `uuid` dependency and generate UUID v4 IDs in `to_eventfold_event`

**Description:**
Add the `uuid` crate (v1, feature `v4`) to `Cargo.toml` and update `to_eventfold_event` in
`src/aggregate.rs` to call `.with_id(Uuid::new_v4().to_string())` immediately after
`eventfold::Event::new(...)`. Add unit tests asserting that the returned event has a `Some` ID
matching the UUID v4 regex, that two successive calls produce distinct IDs, and that the reducer
still applies events from a pre-existing log entry with `id: None` without error.

**Scope:**
- Modify: `Cargo.toml` — add `uuid = { version = "1", features = ["v4"] }` to `[dependencies]`
- Modify: `src/aggregate.rs` — add `use uuid::Uuid;` import; chain `.with_id(Uuid::new_v4().to_string())` after `Event::new` in `to_eventfold_event`; add three new `#[test]` functions inside the existing `mod tests` block

**Acceptance Criteria:**
- [ ] `Cargo.toml` `[dependencies]` contains `uuid = { version = "1", features = ["v4"] }` with no other uuid features
- [ ] `to_eventfold_event` returns an `eventfold::Event` whose `id` field is `Some(s)` where `s` matches the UUID v4 regex `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`
- [ ] Two successive calls to `to_eventfold_event` with identical arguments produce events with different `id` values
- [ ] A hand-constructed `eventfold::Event` with `id: None` (simulating a pre-existing log record) is still applied correctly by `reducer::<Counter>()` without error, leaving state unchanged for an unknown type and advancing state for a known type
- [ ] `cargo test` passes with no failures

**Dependencies:** None
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 2, AC 3, AC 7, AC 8

---

### Ticket 2: Verification and integration check

**Description:**
Run the complete PRD acceptance criteria checklist end-to-end. Confirm the build is warning-free,
clippy is clean, formatting is correct, and all existing and new tests pass. Verify the UUID v4
format and uniqueness properties by reviewing the test output.

**Scope:**
- No file changes expected; this ticket runs verification commands only.
- If any check fails, minimal targeted fixes are permitted in `Cargo.toml` or `src/aggregate.rs`.

**Acceptance Criteria:**
- [ ] `cargo test` passes with no failures (all pre-existing tests and the new ID tests pass)
- [ ] `cargo build` produces no compiler warnings
- [ ] `cargo clippy -- -D warnings` produces no warnings or errors
- [ ] `cargo fmt --check` reports no formatting violations
- [ ] `grep -E 'uuid' Cargo.toml` shows the `uuid` entry with `features = ["v4"]` and nothing else
- [ ] All PRD acceptance criteria (AC 1 through AC 8) are satisfied as confirmed by the test suite and manual review

**Dependencies:** Ticket 1
**Complexity:** S

---

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1 | `to_eventfold_event` returns `id: Some(s)` where `s` is a valid UUID v4 string | Ticket 1, Ticket 2 | Covered |
| 2 | Two successive calls produce different `id` values | Ticket 1, Ticket 2 | Covered |
| 3 | `cargo test` passes with no failures | Ticket 1, Ticket 2 | Covered |
| 4 | `cargo build` produces no compiler warnings | Ticket 2 | Covered |
| 5 | `cargo clippy -- -D warnings` produces no warnings or errors | Ticket 2 | Covered |
| 6 | `cargo fmt --check` reports no formatting violations | Ticket 2 | Covered |
| 7 | `Cargo.toml` contains `uuid` with `v4` feature and nothing beyond what is necessary | Ticket 1, Ticket 2 | Covered |
| 8 | Events loaded from a pre-existing log with `id: None` still apply correctly via `reduce` | Ticket 1, Ticket 2 | Covered |
