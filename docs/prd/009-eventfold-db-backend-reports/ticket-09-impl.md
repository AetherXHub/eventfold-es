# Implementation Report: Ticket 9 -- src/projection.rs: rewrite to gRPC SubscribeAll cursor

**Ticket:** 9 - src/projection.rs: rewrite to gRPC SubscribeAll cursor
**Date:** 2026-02-27 18:00
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/projection.rs` - Full rewrite: replaced per-stream byte-offset cursors with a single `GlobalCursor` (resume token) backed by `client.subscribe_all_from()`. Simplified `Projection` trait by removing `subscriptions()` and changing `apply` to accept `&StoredEvent`. Rewrote `ProjectionCheckpoint<P>` to use `last_global_position: u64` instead of `cursors: HashMap`. Rewrote `ProjectionRunner<P>` to hold `client: EsClient` and `checkpoint_dir: PathBuf`. Added `catch_up()` method that processes a `SubscribeAll` stream. Extracted `process_stream()` for testability with mock streams.
- `src/lib.rs` - Uncommented `mod projection;` and added `pub use projection::Projection;`.

## Implementation Notes

- **Resume token semantics**: `last_global_position` is a resume token (next position to read from), not the position of the last processed event. After processing an event at global position N, it is set to N + 1. This avoids the ambiguity between "fresh, never ran" (position 0) and "processed event at position 0" (position 1). The `subscribe_all_from` call always passes `last_global_position` directly.

- **Skipped events advance the cursor**: When a `RecordedEvent` has missing/unparseable metadata (non-eventfold-es event), `decode_stored_event` returns `None` and the event is not applied. However, `last_global_position` still advances past the event to prevent re-reading it on subsequent catch-ups.

- **Testability via `process_stream`**: The stream processing logic is extracted into a private `async fn process_stream()` that accepts `impl Stream<Item = Result<SubscribeResponse, tonic::Status>> + Unpin`. Tests construct mock streams via `tokio_stream::iter()` without needing a live gRPC server. The `catch_up()` method delegates to `process_stream()` after opening the subscription.

- **`#![allow(dead_code)]` at module level**: The `ProjectionRunner`, `ProjectionCheckpoint`, `save_checkpoint`, and `load_checkpoint` are `pub(crate)` but not yet consumed by any other module (the `store` module is still commented out). This follows the same pattern as the `storage` module.

- **Concurrent ticket conflict**: Another ticket added `src/actor.rs` and wired `mod actor;` into `lib.rs`, but that file has compilation errors (missing `Clone` bound on `A::Command`, missing proto fields). The linter auto-wires `mod actor;` on every edit to `lib.rs`. The actor lines are commented out in the committed state. The actor ticket should handle its own `lib.rs` wiring when it is complete.

## Acceptance Criteria

- [x] AC 1: Projection trait loses subscriptions() method; apply signature changes to `fn apply(&mut self, event: &StoredEvent)` - The trait now has only `NAME` and `apply(&mut self, event: &StoredEvent)`.
- [x] AC 2: ProjectionCheckpoint<P> replaces cursors: HashMap with last_global_position: u64; serializes to { "state": ..., "last_global_position": N } - `ProjectionCheckpoint` has `state: P` and `last_global_position: u64`, verified by `checkpoint_serializes_with_last_global_position` test.
- [x] AC 3: ProjectionRunner<P> holds client: EsClient and checkpoint_dir: PathBuf (no more StreamLayout) - `ProjectionRunner` holds `client: crate::client::EsClient` and `checkpoint_dir: PathBuf`.
- [x] AC 4: ProjectionRunner::new(client: EsClient, checkpoint_dir: PathBuf) loads checkpoint from disk or defaults to position 0 - `new()` accepts `client` and `checkpoint_dir`, calls `load_checkpoint()`, defaults to position 0.
- [x] AC 5: ProjectionRunner::catch_up(&mut self) calls client.subscribe_all_from(last_global_position), reads until CaughtUp, for each RecordedEvent calls decode_stored_event (skips None), calls projection.apply(&stored_event), updates last_global_position, then saves checkpoint - Implemented via `catch_up()` delegating to `process_stream()`.
- [x] AC 6: Events with missing or unparseable metadata (non-ES events) are silently skipped - `decode_stored_event` returns `None` for such events; cursor still advances.
- [x] AC 7: Test: catch_up on fresh checkpoint with mock stream returning 2 events + CaughtUp results in count == 2 and correct last_global_position - `catch_up_fresh_checkpoint_with_two_events` test.
- [x] AC 8: Test: second catch_up with only CaughtUp leaves count unchanged - `second_catch_up_with_only_caught_up_leaves_count_unchanged` test.
- [x] AC 9: Test: RecordedEvent with metadata = b"{}" is skipped - `recorded_event_with_empty_metadata_is_skipped` test.
- [x] AC 10: Test: checkpoint save/load roundtrip - `checkpoint_save_load_roundtrip_via_process_stream` test (also `save_then_load_roundtrips` and `checkpoint_serde_roundtrip`).
- [x] AC 11: Quality gates pass - See test results below.

## Test Results

- Lint (clippy): PASS (`cargo clippy -- -D warnings` produces no errors)
- Formatting: PASS (`cargo fmt --check` on modified files produces no diffs)
- Tests: PASS (78 tests + 3 doctests, all passing)
- Build: PASS (`cargo build` completes with zero warnings)
- New tests added:
  - `src/projection.rs::tests::projection_trait_has_no_subscriptions_method`
  - `src/projection.rs::tests::checkpoint_serializes_with_last_global_position`
  - `src/projection.rs::tests::checkpoint_default_has_position_zero`
  - `src/projection.rs::tests::checkpoint_serde_roundtrip`
  - `src/projection.rs::tests::save_then_load_roundtrips`
  - `src/projection.rs::tests::load_from_empty_dir_returns_none`
  - `src/projection.rs::tests::corrupt_file_returns_none`
  - `src/projection.rs::tests::save_creates_parent_dir`
  - `src/projection.rs::tests::catch_up_fresh_checkpoint_with_two_events`
  - `src/projection.rs::tests::second_catch_up_with_only_caught_up_leaves_count_unchanged`
  - `src/projection.rs::tests::recorded_event_with_empty_metadata_is_skipped`
  - `src/projection.rs::tests::checkpoint_save_load_roundtrip_via_process_stream`

## Concerns / Blockers

- **Concurrent actor.rs ticket**: `src/actor.rs` exists on disk with compilation errors (from another ticket). Its `mod actor;` and `pub use actor::AggregateHandle;` lines are kept commented out in `lib.rs`. A linter/rust-analyzer auto-wires them on edits; this is suppressed by commenting them out. The actor ticket implementer should handle wiring when their code compiles cleanly.
- **`last_global_position` is a resume token**: Downstream tickets should be aware that `last_global_position` stores `global_position + 1` (the next position to read from), not the raw position of the last event. This simplifies the subscribe call but means the stored value is one higher than the actual last event's position.
