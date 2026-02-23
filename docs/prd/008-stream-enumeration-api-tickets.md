# Tickets for PRD 008: AggregateStore Stream Enumeration API

**Source PRD:** docs/prd/008-stream-enumeration-api.md
**Created:** 2026-02-22
**Total Tickets:** 3
**Estimated Total Complexity:** 5 (Ticket 1 = S(1), Ticket 2 = M(2), Ticket 3 = M(2))

---

### Ticket 1: Add `list_aggregate_types` helper to `StreamLayout`

**Description:**
Add a `pub(crate)` method `list_aggregate_types` to `StreamLayout` in `src/storage.rs` that reads
`<base_dir>/streams/` and returns a sorted `Vec<String>` of subdirectory names, one per aggregate
type. This is the only new building block required by `list_streams(None)` and must exist before
`AggregateStore` can call it. Include unit tests in the existing `#[cfg(test)]` module in
`src/storage.rs`.

**Scope:**
- Modify: `src/storage.rs` (add `list_aggregate_types` method to `impl StreamLayout` and 2 unit tests)

**Acceptance Criteria:**
- [ ] `StreamLayout::list_aggregate_types()` compiles with `pub(crate)` visibility.
- [ ] Called on a store with `streams/counter/` and `streams/toggle/` directories present,
      it returns `["counter", "toggle"]` in sorted lexicographic order.
- [ ] Called on a store where the `streams/` directory does not exist yet, it returns
      `Ok(vec![])` (not an error).
- [ ] Called on a store where `streams/` exists but is empty, it returns `Ok(vec![])`.
- [ ] The method doc comment includes `# Arguments`, `# Returns`, and `# Errors` sections
      consistent with the existing `list_streams` doc style in the same file.
- [ ] `cargo test -p eventfold-es storage` passes with no new failures or warnings.

**Dependencies:** None
**Complexity:** S
**Maps to PRD AC:** AC 1 (partial — foundational piece for the `None` path), AC 3, AC 4

---

### Ticket 2: Implement `list_streams` and `read_events` on `AggregateStore`

**Description:**
Add two new `pub async fn` methods to `impl AggregateStore` in `src/store.rs`:
`list_streams(aggregate_type: Option<&str>) -> io::Result<Vec<(String, String)>>` and
`read_events(aggregate_type: &str, instance_id: &str) -> io::Result<Vec<eventfold::Event>>`.
Both delegate blocking I/O to `tokio::task::spawn_blocking`, following the established pattern
of the existing `list<A>` method. Include unit tests in the existing `#[cfg(test)]` module in
`src/store.rs` covering all seven behavioural ACs from the PRD.

**Scope:**
- Modify: `src/store.rs` (add two methods to `impl AggregateStore` and 7 unit tests)

**Acceptance Criteria:**
- [ ] `list_streams(None)` on a store with `counter` instances `["c-1", "c-2"]` and `toggle`
      instance `["t-1"]` returns
      `[("counter", "c-1"), ("counter", "c-2"), ("toggle", "t-1")]` in that exact order
      (primary sort by aggregate type, secondary by instance ID).
- [ ] `list_streams(Some("counter"))` on the same store returns exactly
      `[("counter", "c-1"), ("counter", "c-2")]`.
- [ ] `list_streams(None)` on a freshly opened store returns `Ok(vec![])`.
- [ ] `list_streams(Some("nonexistent"))` returns `Ok(vec![])`.
- [ ] `read_events("counter", "c-1")` after two `Increment` commands returns a `Vec` of
      length 2 where both events have `event_type == "Incremented"`.
- [ ] `read_events` on a stream whose directory exists (via `store.get::<Counter>("c-1")`)
      but with no commands executed returns `Ok(vec![])`.
- [ ] `read_events("nonexistent", "x")` where no directory was ever created returns
      `Err(e)` with `e.kind() == io::ErrorKind::NotFound`.
- [ ] Both methods have doc comments with `# Arguments`, `# Returns`, and `# Errors` sections
      matching the style of the existing `get` and `list` methods in `src/store.rs`.
- [ ] `cargo test -p eventfold-es store` passes with no new failures or warnings.

**Dependencies:** Ticket 1 (requires `StreamLayout::list_aggregate_types`)
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 2, AC 3, AC 4, AC 5, AC 6, AC 7, AC 10

---

### Ticket 3: Verification and Integration

**Description:**
Run the full PRD acceptance criteria checklist end-to-end. Verify all tickets integrate
correctly as a cohesive feature: the new methods are visible from the public API via
`AggregateStore`, the codebase compiles without warnings, Clippy passes, and all existing
tests continue to pass alongside the new ones.

**Scope:**
- No new files. Read-only verification pass.

**Acceptance Criteria:**
- [ ] All PRD acceptance criteria pass (ACs 1-10 verified via `cargo test`).
- [ ] No regressions in existing tests: `cargo test` exits 0.
- [ ] `cargo build` exits 0 with no compiler warnings.
- [ ] `cargo clippy -- -D warnings` exits 0 with no new diagnostics.
- [ ] `cargo doc --no-deps` exits 0 (doc comments are valid and render without errors).
- [ ] `list_streams` and `read_events` are accessible as methods on `AggregateStore` in
      downstream code without any additional `use` imports beyond `use eventfold_es::AggregateStore`.

**Dependencies:** Ticket 1, Ticket 2
**Complexity:** M
**Maps to PRD AC:** AC 8, AC 9, AC 10 (full sweep)

---

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1 | `list_streams(None)` on counter + toggle store returns sorted `[(counter,c-1),(counter,c-2),(toggle,t-1)]` | Ticket 1 (helper), Ticket 2 | Covered |
| 2 | `list_streams(Some("counter"))` returns only counter pairs | Ticket 2 | Covered |
| 3 | `list_streams(None)` on empty store returns `Ok(vec![])` | Ticket 1, Ticket 2 | Covered |
| 4 | `list_streams(Some("nonexistent"))` returns `Ok(vec![])` | Ticket 1, Ticket 2 | Covered |
| 5 | `read_events("counter","c-1")` after 2 Increments returns Vec of length 2 with correct event_type | Ticket 2 | Covered |
| 6 | `read_events` on a stream with no commands returns `Ok(vec![])` | Ticket 2 | Covered |
| 7 | `read_events` on a never-created stream returns `Err` with `NotFound` | Ticket 2 | Covered |
| 8 | `cargo test` passes with no new failures or warnings | Ticket 3 | Covered |
| 9 | `cargo clippy -- -D warnings` produces no new diagnostics | Ticket 3 | Covered |
| 10 | Both new methods have doc comments with summary, `# Arguments`, `# Returns`, `# Errors` | Ticket 2, Ticket 3 | Covered |
