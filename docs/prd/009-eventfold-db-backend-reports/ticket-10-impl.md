# Implementation Report: Ticket 10 -- src/process_manager.rs: rewrite to gRPC SubscribeAll cursor

**Ticket:** 10 - src/process_manager.rs: rewrite to gRPC SubscribeAll cursor
**Date:** 2026-02-27 20:45
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/process_manager.rs` - Complete rewrite: removed old JSONL/eventfold-based ProcessManager trait and runner; replaced with gRPC SubscribeAll-backed ProcessManager trait, ProcessManagerCheckpoint with global cursor, ProcessManagerRunner with EsClient, and async ProcessManagerCatchUp trait object interface.
- `src/lib.rs` - Uncommented `mod process_manager;` and added `pub use process_manager::{ProcessManager, ProcessManagerReport};` exports.

## Implementation Notes

- **Mirrored projection.rs pattern closely**: The checkpoint structure (`ProcessManagerCheckpoint<PM>` with `state` + `last_global_position`), the runner construction (`new(client, checkpoint_dir)`), and the stream processing (`process_stream` factored out for testability) all follow the same pattern as `ProjectionRunner<P>`.

- **Key difference from projections**: `catch_up()` returns `Vec<CommandEnvelope>` and does NOT save the checkpoint. The caller is responsible for dispatching envelopes first, then calling `save()`. This ensures crash safety -- a crash mid-dispatch causes re-processing on restart.

- **ProcessManagerCatchUp trait is now async**: The old trait had synchronous `catch_up() -> io::Result<Vec<CommandEnvelope>>`. The new trait returns a boxed future (`Pin<Box<dyn Future<Output = io::Result<Vec<CommandEnvelope>>> + Send + '_>>`) to match the async nature of the gRPC subscribe stream.

- **EnteredSpan Send issue**: The `tracing::debug_span!().entered()` guard is not `Send`, which conflicts with the `ProcessManagerCatchUp` boxed `Send` future. Replaced with a simple `tracing::debug!()` log at the start of `process_stream` instead of holding a span across awaits.

- **AggregateDispatcher and TypedDispatcher omitted**: These types from the old module referenced `crate::store::AggregateStore` which is currently commented out. They are dispatch infrastructure, not part of the PM trait/runner pattern. They will be re-added when the store module is rewritten in a later ticket.

- **test_fixtures module preserved as pub(crate)**: The `EchoSaga` test fixture is available for other test modules (e.g., future store integration tests) via `process_manager::test_fixtures::EchoSaga`.

- **Dead-letter infrastructure retained**: `append_dead_letter()`, `DeadLetterEntry`, and `ProcessManagerReport` are kept as-is since they are independent of the storage backend.

## Acceptance Criteria

- [x] AC 1: ProcessManager trait loses subscriptions() method; react signature changes to `fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>` - Implemented in the trait definition (lines 47-67).
- [x] AC 2: ProcessManagerCheckpoint<PM> uses last_global_position: u64 (same structure as ProjectionCheckpoint) - Implemented as `ProcessManagerCheckpoint<PM>` with `state: PM` and `last_global_position: u64` (lines 74-92).
- [x] AC 3: ProcessManagerRunner<PM> holds client: EsClient and checkpoint_dir: PathBuf - Implemented in the struct definition (lines 170-177).
- [x] AC 4: catch_up consumes SubscribeAll stream until CaughtUp, calls decode_stored_event (skips None), calls pm.react(&stored_event), collects envelopes; does NOT save checkpoint (caller calls save after dispatch) - Implemented in `catch_up()` and `process_stream()` (lines 234-297).
- [x] AC 5: save persists last_global_position and PM state - Implemented via `save()` delegating to `save_pm_checkpoint()` (lines 299-309).
- [x] AC 6: ProcessManagerCatchUp trait object interface updated to match new signatures - Now returns boxed async future for `catch_up()` (lines 329-369).
- [x] AC 7: Test: catch_up on mock stream with one valid ES event returns vec![CommandEnvelope] of length 1 and updates last_global_position - `catch_up_with_one_valid_event_returns_one_envelope` (line 604).
- [x] AC 8: Test: second catch_up with saved checkpoint returns empty Vec - `second_catch_up_with_saved_checkpoint_returns_empty` (line 621).
- [x] AC 9: Test: non-ES events skipped; catch_up returns empty Vec - `non_es_events_skipped_returns_empty` (line 643).
- [x] AC 10: Test: ProcessManagerCheckpoint serialization roundtrip - `checkpoint_serialization_roundtrip` (line 542).
- [x] AC 11: Quality gates pass - All four gates pass (see below).

## Test Results

- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (91 unit tests + 3 doctests, all passing; 10 new tests in process_manager module)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt --check` -- no diffs)
- New tests added:
  - `src/process_manager.rs::tests::process_manager_trait_has_no_subscriptions_method`
  - `src/process_manager.rs::tests::checkpoint_serialization_roundtrip`
  - `src/process_manager.rs::tests::checkpoint_default_has_position_zero`
  - `src/process_manager.rs::tests::save_then_load_roundtrips`
  - `src/process_manager.rs::tests::load_from_empty_dir_returns_none`
  - `src/process_manager.rs::tests::corrupt_file_returns_none`
  - `src/process_manager.rs::tests::catch_up_with_one_valid_event_returns_one_envelope`
  - `src/process_manager.rs::tests::second_catch_up_with_saved_checkpoint_returns_empty`
  - `src/process_manager.rs::tests::non_es_events_skipped_returns_empty`
  - `src/process_manager.rs::tests::dead_letter_append_creates_readable_jsonl`

## Concerns / Blockers

- **AggregateDispatcher/TypedDispatcher removed**: The old process_manager.rs contained `AggregateDispatcher` trait and `TypedDispatcher<A>` struct that reference `crate::store::AggregateStore`. Since the store module is currently commented out, these types cannot compile. They were omitted from this rewrite. The store module ticket should re-introduce them (they are dispatch infrastructure, not PM-specific).
- **ProcessManagerCatchUp is async now**: The old trait had sync methods. The new trait uses boxed futures for `catch_up()`. Any code that previously called the sync version will need updating (the store module, which is commented out and will be rewritten).
