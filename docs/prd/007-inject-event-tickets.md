# Tickets for PRD 007: inject_event

**Source PRD:** docs/prd/007-inject-event.md
**Created:** 2026-02-22
**Total Tickets:** 3
**Estimated Total Complexity:** 7 (S=1, M=2, L=3 → 3+3+1=7)

---

### Ticket 1: Add `ActorMessage::Inject` variant and handler in `src/actor.rs`

**Description:**
Extend the actor loop to accept a new `Inject` message that appends a pre-validated
`eventfold::Event` directly to the actor's owned `EventWriter`. This keeps the actor
as the sole writer for any stream that has a live actor, eliminating lock contention
entirely. The variant carries the event and a `oneshot::Sender<io::Result<()>>` reply
channel so `inject_event` in `store.rs` can await the result.

**Scope:**
- Modify: `src/actor.rs`
  - Add `ActorMessage::Inject { event: eventfold::Event, reply: oneshot::Sender<io::Result<()>> }`
    to the `ActorMessage<A>` enum.
  - Add a match arm in `run_actor` that calls `writer.append(&event)` and sends the result
    back on `reply`.
  - Add a public(crate) helper `inject_via_actor` on `AggregateHandle<A>` that sends the
    `Inject` message and awaits the reply, mapping a closed channel to
    `io::Error::new(io::ErrorKind::BrokenPipe, "actor gone")`.

**Acceptance Criteria:**
- [ ] `ActorMessage::Inject` compiles without warnings and all existing actor tests pass
      (`cargo test -p eventfold-es actor` produces zero failures).
- [ ] A unit test in `src/actor.rs` verifies that `inject_via_actor` successfully appends
      an event and that `get_state` after injection reflects the injected event in the
      aggregate state (actor replays from its refreshed view on the next `GetState`).
- [ ] `cargo clippy -- -D warnings` produces no new warnings in `src/actor.rs`.

**Dependencies:** None
**Complexity:** L
**Maps to PRD AC:** AC 7

---

### Ticket 2: Add `InjectOptions`, dedup `seen_ids`, and `AggregateStore::inject_event` in `src/store.rs`; re-export from `src/lib.rs`

**Description:**
Implement the full `inject_event` public API on `AggregateStore`. This ticket adds the
`InjectOptions` struct, a `seen_ids: Arc<std::sync::Mutex<HashSet<String>>>` field to
`AggregateStore` (shared across clones via `Arc`), and the async `inject_event` method
that: (1) deduplicates by `event.id`, (2) ensures the stream directory exists,
(3) appends via the live actor if one is cached, or via a temporary `EventWriter` in
`spawn_blocking` if not, (4) registers the ID in `seen_ids` on success,
(5) triggers all registered projection runners via their existing `catch_up` path,
and (6) optionally calls `run_process_managers` when `opts.run_process_managers == true`.
`InjectOptions` is re-exported from `src/lib.rs` with complete doc comments.

**Scope:**
- Modify: `src/store.rs`
  - Add `use std::collections::HashSet;` import.
  - Add `seen_ids: Arc<std::sync::Mutex<HashSet<String>>>` field to `AggregateStore`.
  - Initialize `seen_ids` in both `AggregateStore::open` and `AggregateStoreBuilder::open`.
  - Add `pub struct InjectOptions { pub run_process_managers: bool }` with `Default` impl.
  - Add `pub async fn inject_event<A: Aggregate>(&self, instance_id: &str, event: eventfold::Event, opts: InjectOptions) -> io::Result<()>`.
  - Add unit tests covering AC 1–8 (see Acceptance Criteria below).
- Modify: `src/lib.rs`
  - Re-export `InjectOptions` from `crate::store`.

**Acceptance Criteria:**
- [ ] `store.inject_event::<Counter>("id", event, InjectOptions::default()).await` returns
      `Ok(())` and the event is the last line of the stream's `app.jsonl` (AC 1).
- [ ] Calling `store.projection::<EventCounter>()` after `inject_event` reflects the
      injected event (count incremented by 1) (AC 2).
- [ ] Calling `inject_event` twice with the same `event.id = Some("ev-1")` produces exactly
      one line in `app.jsonl` (the second call is a no-op returning `Ok(())`) (AC 3).
- [ ] Calling `inject_event` twice with events where `event.id = None` produces two lines
      in `app.jsonl` (no dedup for id-less events) (AC 4).
- [ ] With `InjectOptions { run_process_managers: false }` (default), process managers are
      NOT triggered automatically; a subsequent explicit `run_process_managers()` call
      dispatches one envelope per injected event (AC 5).
- [ ] With `InjectOptions { run_process_managers: true }`, a single `inject_event` call
      causes process managers to fire without a separate `run_process_managers()` call (AC 6).
- [ ] Injecting into a stream that has a live actor (cached in `self.cache`) completes
      within 1 second and does not return `WouldBlock` or deadlock (uses `inject_via_actor`
      from Ticket 1) (AC 7).
- [ ] Injecting into a stream with no prior directory creates the directory; a subsequent
      `store.get::<A>("id").await` followed by `handle.state()` reflects the injected
      event in aggregate state (actor replays from JSONL on spawn) (AC 8).
- [ ] `InjectOptions` and `inject_event` are visible from `eventfold_es` crate root and
      carry complete `///` doc comments (AC 10).
- [ ] `cargo test` passes with no new failures.

**Dependencies:** Ticket 1
**Complexity:** L
**Maps to PRD AC:** AC 1, AC 2, AC 3, AC 4, AC 5, AC 6, AC 7, AC 8, AC 10

---

### Ticket 3: Verification and Integration Check

**Description:**
Run the full PRD acceptance criteria checklist end-to-end. Verify that all tickets
integrate correctly and that no regressions exist in the existing test suite. Confirm
linting, formatting, and documentation generation all succeed.

**Acceptance Criteria:**
- [ ] All PRD acceptance criteria pass (ACs 1–10 verified by the tests added in Tickets 1–2).
- [ ] `cargo test` passes with zero failures.
- [ ] `cargo clippy -- -D warnings` passes with zero warnings.
- [ ] `cargo fmt --check` passes with no formatting violations.
- [ ] `cargo doc --no-deps` succeeds; `InjectOptions` and `AggregateStore::inject_event`
      appear in the generated HTML with non-empty doc comments (AC 9, AC 10).
- [ ] No regressions in the existing actor, store, projection, or process-manager test suites.

**Dependencies:** Ticket 1, Ticket 2
**Complexity:** S
**Maps to PRD AC:** AC 9

---

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1        | `inject_event` returns `Ok(())` and event appears as last line of `app.jsonl` | Ticket 2 | Covered |
| 2        | Registered projection reflects injected event after call returns | Ticket 2 | Covered |
| 3        | Duplicate `inject_event` with same `event.id = Some("ev-1")` produces exactly one JSONL line | Ticket 2 | Covered |
| 4        | Two calls with `event.id = None` both write (no dedup for id-less events) | Ticket 2 | Covered |
| 5        | `InjectOptions { run_process_managers: false }` (default) does NOT auto-trigger PMs; explicit `run_process_managers()` dispatches expected envelopes | Ticket 2 | Covered |
| 6        | `InjectOptions { run_process_managers: true }` triggers PMs without a separate call | Ticket 2 | Covered |
| 7        | Injecting into a stream with a live actor completes within 1s without deadlock or `WouldBlock` | Ticket 1, Ticket 2 | Covered |
| 8        | Injecting into a new stream creates the directory; subsequent `get` + `state()` reflects the event | Ticket 2 | Covered |
| 9        | `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass | Ticket 3 | Covered |
| 10       | `InjectOptions` and `inject_event` re-exported from `lib.rs` with complete doc comments | Ticket 2, Ticket 3 | Covered |
