# Implementation Report: Ticket 1 -- Add `ActorMessage::Inject` variant and handler in `src/actor.rs`

**Ticket:** 1 - Add `ActorMessage::Inject` variant and handler in `src/actor.rs`
**Date:** 2026-02-22 18:30
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/actor.rs` - Added `ActorMessage::Inject` variant, handler in `run_actor` match, `inject_via_actor` method on `AggregateHandle`, and unit test

## Implementation Notes
- The `Inject` variant carries `event: eventfold::Event` and `reply: oneshot::Sender<io::Result<()>>`, matching the ticket specification exactly.
- The `run_actor` match arm calls `writer.append(&event).map(|_| ())` to discard the `AppendResult` and return only the I/O result, then sends it back on the reply channel.
- The `inject_via_actor` method follows the exact same request/reply pattern used by `execute()` and `state()` -- sends a message via the mpsc sender, awaits the oneshot reply.
- Channel errors (both `send` and `recv`) are mapped to `io::Error::new(io::ErrorKind::BrokenPipe, "actor gone")` as specified.
- Both `ActorMessage::Inject` and `inject_via_actor` have `#[allow(dead_code)]` annotations because their consumer (`store.rs` in ticket 2) has not been implemented yet. This follows the existing pattern used by `ActorMessage::Shutdown`.
- The `GetState` handler already calls `view.refresh(reader)` before returning state, so after an `Inject` call, a subsequent `state()` call will see the injected event reflected in the aggregate state. The test verifies this end-to-end.

## Acceptance Criteria
- [x] AC 1: `ActorMessage::Inject` compiles without warnings and all existing actor tests pass - All 94 original tests pass, clippy clean with `-D warnings`
- [x] AC 2: Unit test verifies `inject_via_actor` appends event and `get_state` reflects it - `inject_via_actor_appends_and_updates_state` test passes, asserting counter value is 1 after injecting an `Incremented` event
- [x] AC 3: `cargo clippy -- -D warnings` produces no new warnings in `src/actor.rs` - Clean pass

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` - zero warnings)
- Tests: PASS (95 unit tests + 7 doc-tests, all passing)
- Build: PASS (`cargo build` - zero warnings)
- Format: PASS (`cargo fmt --check` - no issues)
- New tests added: `src/actor.rs::tests::inject_via_actor_appends_and_updates_state`

## Concerns / Blockers
- The `#[allow(dead_code)]` annotations on `Inject` and `inject_via_actor` should be removed by ticket 2 when `store.rs` begins consuming these APIs. If they are not removed, they will be harmless but unnecessary.
- None otherwise.
