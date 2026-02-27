# Implementation Report: Ticket 8 -- src/actor.rs: full rewrite to gRPC-backed actor loop

**Ticket:** 8 - src/actor.rs: full rewrite to gRPC-backed actor loop
**Date:** 2026-02-27 15:30
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/actor.rs` - Full rewrite: replaced eventfold::{EventWriter, EventReader, View} with gRPC-backed actor loop using EventStoreOps trait, EsClient, and snapshot module. Actor now runs as async tokio task instead of blocking thread.
- `src/lib.rs` - Uncommented `mod actor;` and added `pub use actor::AggregateHandle;`
- `src/aggregate.rs` - Added `#[derive(Clone)]` to `CounterCommand` test fixture to support retry-on-conflict (Command must be cloneable for re-handle after version conflicts).

## Implementation Notes

- **EventStoreOps trait**: Introduced a `pub(crate)` trait abstracting `append()` and `read_stream()` operations. `EsClient` implements this for production use; tests provide a `MockStore` implementation. This avoids adding new dependencies (no mockall/etc.) and uses `tonic::async_trait` which tonic re-exports.

- **Async actor loop**: The actor now runs as a `tokio::spawn`ed async task (was a `std::thread::spawn` blocking thread). This eliminates the need for a nested runtime and aligns with the async gRPC client.

- **ActorContext struct**: Grouped 5 parameters (store, stream_id, instance_id, base_dir, config) into `ActorContext<S>` to keep `run_actor` within clippy's 7-argument limit.

- **stream_version convention**: `stream_version` represents the zero-based version of the last event applied (matching `RecordedEvent.stream_version`). For a fresh aggregate with no events, starts at 0. After the first event at version 0, stores 0. OCC uses `Exact(stream_version)`. Catch-up reads from `stream_version + 1`.

- **tracing::Instrument**: Used `Instrument::instrument()` instead of `span.entered()` to avoid holding a non-Send `EnteredSpan` across await points.

- **Clone bound on Command**: Added `A::Command: Clone` as a where clause on `spawn_actor_with_config`, `spawn_actor_with_store`, `run_actor`, and `execute_with_retry`. This is required because retry-on-conflict needs to re-execute the command against updated state. The `Aggregate` trait itself was not modified (as per ticket scope), but the test fixture `CounterCommand` needed `#[derive(Clone)]`.

- **#![allow(dead_code)]**: Added at module level, following the established pattern in `snapshot.rs`, because the actor module's `pub(crate)` items are not yet consumed by the store module (to be rewritten in a future ticket).

## Acceptance Criteria

- [x] AC 1: ActorMessage<A>::Inject variant changed from `event: eventfold::Event` to `proposed: ProposedEventData`; reply type is `oneshot::Sender<Result<(), tonic::Status>>`
- [x] AC 2: AggregateHandle<A> no longer holds an EventReader; reader() accessor removed; is_alive(), execute(), state() remain
- [x] AC 3: spawn_actor_with_config<A> signature changed to (instance_id: &str, client: EsClient, base_dir: &Path, config: ActorConfig) -> AggregateHandle<A>
- [x] AC 4: Public spawn_actor(stream_dir) function removed from public API (entirely deleted)
- [x] AC 5: Actor loop on spawn: loads Snapshot<A> via load_snapshot, derives stream_id via stream_uuid, calls client.read_stream(stream_id, snapshot.stream_version + 1, u64::MAX) and folds each event through A::apply
- [x] AC 6: Actor loop on Execute: encodes via encode_domain_event, calls client.append(stream_id, Exact(version), events), on FAILED_PRECONDITION re-reads and retries up to MAX_RETRIES = 3; on 3rd failure returns Err(ExecuteError::WrongExpectedVersion)
- [x] AC 7: Actor loop on idle timeout or Shutdown: saves snapshot via save_snapshot before exiting
- [x] AC 8: Test: 3 successive FAILED_PRECONDITION errors -> execute() returns Err(ExecuteError::WrongExpectedVersion) -- `three_precondition_failures_returns_wrong_expected_version`
- [x] AC 9: Test: execute Increment, Shutdown, load_snapshot shows stream_version == 0 and state.value == 1 -- `execute_then_shutdown_saves_snapshot`
- [x] AC 10: Test: pre-existing snapshot at stream_version = 2, value = 2, mock returns Incremented at version 3, after spawn state().await returns Counter { value: 3 } -- `spawn_with_snapshot_catches_up_from_server`
- [x] AC 11: Quality gates pass

## Test Results

- Lint (clippy): PASS
- Fmt: PASS
- Build: PASS (zero warnings)
- Tests: PASS (81 tests + 3 doctests, all passing)
- New tests added:
  - `src/actor.rs::tests::three_precondition_failures_returns_wrong_expected_version`
  - `src/actor.rs::tests::execute_then_shutdown_saves_snapshot`
  - `src/actor.rs::tests::spawn_with_snapshot_catches_up_from_server`

## Concerns / Blockers

- **CounterCommand Clone**: Added `#[derive(Clone)]` to `CounterCommand` in `src/aggregate.rs` test fixtures. This is a minimal change outside the stated scope (`src/actor.rs`), but is necessary because the retry-on-conflict pattern requires `Command: Clone`. The actual `Aggregate` trait was not changed. Downstream tickets that define concrete aggregates will need their Command types to implement Clone.

- **snapshot.rs dead_code suppression**: The `#![allow(dead_code)]` comment in `snapshot.rs` says "until the actor module is rewritten to use snapshots" -- this is now resolved since actor.rs imports and uses `load_snapshot` and `save_snapshot`. A future cleanup could remove that suppression.

- **Linter interference**: The IDE's rust-analyzer kept commenting out `mod actor;` in lib.rs whenever it detected compilation errors during development. The final state has `mod actor;` uncommented and compiling cleanly, but if a linter runs between file writes it may revert this line. The quality gates confirm the correct state.
