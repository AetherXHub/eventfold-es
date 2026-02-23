# Code Review: Ticket 1 -- Add `ActorMessage::Inject` variant and handler in `src/actor.rs`

**Ticket:** 1 -- Add `ActorMessage::Inject` variant and handler in `src/actor.rs`
**Impl Report:** docs/prd/007-inject-event-reports/ticket-01-impl.md
**Date:** 2026-02-22 19:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `ActorMessage::Inject` compiles without warnings and all existing actor tests pass | Met | `cargo clippy -- -D warnings` passes cleanly. All 94 pre-existing unit tests + 7 doc-tests pass unchanged. Confirmed by running tests on the pre-change commit (94 unit tests) and post-change (95 unit tests = 94 + 1 new). |
| 2 | Unit test verifies `inject_via_actor` appends event and `get_state` reflects it | Met | Test `inject_via_actor_appends_and_updates_state` (line 539) constructs a raw `eventfold::Event::new("Incremented", Value::Null)`, calls `inject_via_actor`, then asserts `state.value == 1`. This exercises the full path: actor receives `Inject`, appends via `writer.append`, subsequent `GetState` calls `view.refresh(reader)` which picks up the new event, and the counter reducer applies it. |
| 3 | `cargo clippy -- -D warnings` produces no new warnings | Met | Verified: clippy passes with zero warnings on the full crate. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- None

## Suggestions (non-blocking)

- The `#[allow(dead_code)]` annotations on `Inject` (line 69) and `inject_via_actor` (line 301) are correctly justified in the impl report -- the consumer (`store.rs`) is introduced in Ticket 2. The impl report notes these should be removed when Ticket 2 lands. This is good practice and matches the existing pattern on `Shutdown` (line 78) and `DEFAULT_MAX_RETRIES` (line 26).

## Scope Check
- Files within scope: YES -- Only `src/actor.rs` was modified (implementation code). `docs/prd/007-inject-event.md` received a one-line status change (DRAFT -> TICKETS READY), which is standard orchestration bookkeeping.
- Scope creep detected: NO
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- The change is purely additive. A new enum variant, a new match arm, a new method, and a new test. No existing code paths were modified. All 94 pre-existing tests continue to pass.
- Security concerns: NONE
- Performance concerns: NONE -- The `Inject` handler is a thin passthrough to `writer.append()`, identical in cost to the existing `Execute` path's append step.

## Implementation Quality Notes

The implementation is clean and follows the established codebase patterns precisely:

1. **`ActorMessage::Inject` variant** (lines 64-75): Correctly typed with `event: eventfold::Event` and `reply: oneshot::Sender<io::Result<()>>`. Doc comments follow the same style as `Execute` and `GetState` variants. The `#[allow(dead_code)]` annotation parallels the existing one on `Shutdown`.

2. **`run_actor` match arm** (lines 138-141): `writer.append(&event).map(|_| ())` correctly discards the `AppendResult` (matching the ticket spec "sends the result back on `reply`"). `let _ = reply.send(result)` matches the existing pattern for silently discarding closed-receiver errors.

3. **`inject_via_actor` method** (lines 286-310): Follows the exact same request/reply pattern as `execute()` and `state()`. Both channel error paths correctly map to `io::Error::new(io::ErrorKind::BrokenPipe, "actor gone")` as specified. Visibility is `pub(crate)` which is appropriate since only `store.rs` (Ticket 2) will call it. Doc comments are thorough and follow the `# Arguments` / `# Errors` convention from `CLAUDE.md`.

4. **Test** (lines 539-558): Follows Arrange-Act-Assert. Uses `eventfold::Event::new("Incremented", serde_json::Value::Null)` which constructs an event without an ID (matching the `id: None` path). The assertion verifies end-to-end: inject -> actor append -> view refresh -> state reflects the event.

## Verification Commands Run
- `cargo clippy -- -D warnings` -- PASS (zero warnings)
- `cargo fmt --check` -- PASS (no formatting issues)
- `cargo test` -- PASS (95 unit tests + 7 doc-tests, all passing)
- `git diff HEAD --name-only` -- confirmed only `src/actor.rs` and `docs/prd/007-inject-event.md` changed
- Pre-change test count verified: 94 unit tests (stash/unstash confirmation)
