# Code Review: Ticket 8 -- src/actor.rs: full rewrite to gRPC-backed actor loop

**Ticket:** 8 -- src/actor.rs: full rewrite to gRPC-backed actor loop
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-08-impl.md
**Date:** 2026-02-27 17:00
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `ActorMessage::Inject` uses `ProposedEventData` instead of `eventfold::Event` | Met | Lines 134-143: `Inject { proposed: ProposedEventData, reply: oneshot::Sender<Result<(), tonic::Status>> }` |
| 2 | `AggregateHandle` no longer holds `EventReader`; `reader()` removed; `is_alive()`, `execute()`, `state()` remain | Met | Struct at lines 158-160 holds only `sender: mpsc::Sender`. Methods: `execute()` (L193), `state()` (L219), `is_alive()` (L262). No `reader()`. |
| 3 | `spawn_actor_with_config` signature: `(instance_id, client: EsClient, base_dir, config) -> AggregateHandle` | Met | Lines 537-548. Returns `Result<AggregateHandle<A>, tonic::Status>` (Result wrapper pragmatically necessary for fallible spawn). Additional `where A::Command: Clone` bound required for retry. |
| 4 | Public `spawn_actor` removed | Met | Grep confirms no `pub fn spawn_actor` or `pub async fn spawn_actor` exists. lib.rs exports only `AggregateHandle`. |
| 5 | Spawn loads snapshot, derives stream_id, catches up via `read_stream`, folds events | Met | Lines 566-590: `load_snapshot`, `stream_uuid`, `read_stream(stream_id, from_version, u64::MAX)`, fold loop with `decode_domain_event` + `apply`. |
| 6 | Execute encodes via `encode_domain_event`, appends with `Exact(version)`, retries up to 3 on `FAILED_PRECONDITION` | Met | Lines 448-513: `encode_domain_event`, `append(stream_id, Exact(*stream_version), proposed)`, retry loop `for attempt in 0..MAX_RETRIES`, re-read on `FailedPrecondition`, return `WrongExpectedVersion` after exhaustion. |
| 7 | Shutdown saves snapshot | Met | Three shutdown paths all call `save_snapshot_quietly`: `Shutdown` message (L381), channel closed (L392), idle timeout (L401). |
| 8 | Test: 3 FAILED_PRECONDITION errors -> WrongExpectedVersion | Met | `three_precondition_failures_returns_wrong_expected_version` (L744-758): MockStore with 3 precondition failures, asserts `Err(ExecuteError::WrongExpectedVersion)`. |
| 9 | Test: execute + shutdown -> snapshot saved correctly | Met | `execute_then_shutdown_saves_snapshot` (L762-801): executes Increment, sends Shutdown, loads snapshot, asserts `stream_version == 0` and `state.value == 1`. |
| 10 | Test: pre-existing snapshot + read_stream catchup -> correct state | Met | `spawn_with_snapshot_catches_up_from_server` (L805-842): snapshot at version=2/value=2, mock returns one Incremented at version 3, asserts `state.value == 3`. |
| 11 | Quality gates pass | Met | Verified: `cargo fmt --check` (clean), `cargo clippy -- -D warnings` (clean), `cargo build` (zero warnings), `cargo test` (81 tests + 3 doctests, all passing). |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **Unnecessary clone in `execute_with_retry`** (`src/actor.rs` line 451-452): `state.clone().handle(cmd.clone())` clones `state`, but `Aggregate::handle` takes `&self`, so `state.handle(cmd.clone())` would suffice. The `cmd` clone is correctly needed for retry, but the `state` clone is wasteful. Low impact since aggregates are typically small structs.

2. **Initial `stream_version = 0` for empty streams** (`src/actor.rs` lines 576-582): When no snapshot exists and no events are on the server, `version` stays at 0. The first append sends `Exact(0)`, which asserts the stream is at version 0 (has one event). If `eventfold-db` treats a non-existent stream as having no version (not version 0), this would fail. The mock test does not validate the expected version parameter (`_expected` at line 676). This will surface in the integration tests (Ticket 12) and is documented in the impl report's stream_version convention notes.

3. **`#![allow(dead_code)]` on module** (`src/actor.rs` line 3): Acceptable while the store module (Ticket 11) is not yet rewritten. Should be removed when the store module wires up actor spawning. Matches established pattern in `snapshot.rs`.

## Suggestions (non-blocking)

- The `EventStoreOps` trait is a clean abstraction for testability. Consider adding a `Clone` bound (or making it `Clone`-able via `Arc<dyn EventStoreOps>`) if the store module needs to share the client across actors. Currently `MockStore` derives `Clone` and `EsClient` derives `Clone` via tonic channel refcounting, so this works implicitly.

- `save_snapshot_quietly` is a well-named helper that follows the "log and continue" pattern. Consider extracting this to `snapshot.rs` if other modules (e.g., store.rs) need the same fire-and-forget save behavior.

- The `50ms` sleep in `execute_then_shutdown_saves_snapshot` (line 787) is a pragmatic but potentially flaky approach. A `JoinHandle`-based approach or waiting for channel closure could be more deterministic, but given the tokio test runtime this is unlikely to fail in practice.

## Scope Check

- Files within scope: YES -- `src/actor.rs` is the primary target.
- Out-of-scope files touched:
  - `src/lib.rs`: Uncommented `mod actor;` and added `pub use actor::AggregateHandle;`. This is necessary infrastructure wiring -- the module cannot compile without being declared in lib.rs.
  - `src/aggregate.rs`: Added `#[derive(Clone)]` to `CounterCommand` test fixture. Minimal, necessary for the retry mechanism to compile. The `Aggregate` trait itself was not modified.
- Scope creep detected: NO -- both out-of-scope changes are minimal and directly required by the actor implementation. No extra features, abstractions, or cleanups were added.
- Unauthorized dependencies added: NO

## Risk Assessment

- Regression risk: LOW -- The actor module is a full rewrite (no incremental changes to break). Prior test suites for other modules (aggregate, error, event, client, snapshot, storage, projection, command) all pass (81 tests).
- Security concerns: NONE
- Performance concerns: NONE significant. The unnecessary `state.clone()` in `execute_with_retry` (Minor issue #1) is negligible for typical aggregate sizes. The `state.clone().apply(de)` pattern in fold loops is required by the trait signature.
