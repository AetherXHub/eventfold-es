# Implementation Report: Ticket 2 -- Add `InjectOptions`, dedup `seen_ids`, and `AggregateStore::inject_event`

**Ticket:** 2 - Add `InjectOptions`, dedup `seen_ids`, and `AggregateStore::inject_event`
**Date:** 2026-02-22 21:45
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/store.rs` - Added `InjectOptions` struct, `seen_ids` field, `projection_catch_ups` field, `ProjectionCatchUpFn` trait, `SharedProjectionCatchUp` wrapper, `inject_event` method, and 8 unit tests. Updated `AggregateStore::open`, builder's `open`, `projection()`, and `rebuild_projection()` to work with the new `Arc<Mutex<ProjectionRunner<P>>>` shared storage.
- `src/lib.rs` - Added `InjectOptions` to the re-export line.

## Implementation Notes

### Key Design Decision: Type-erased projection catch-up

The ticket requires `inject_event` to "trigger projection catch-up for all registered projections." However, the existing `ProjectionMap` stores runners as `Box<dyn Any + Send + Sync>`, which cannot call `catch_up()` without knowing the concrete `P` type parameter.

To solve this without modifying `src/projection.rs` (out of scope), I added:
1. A `ProjectionCatchUpFn` trait (private to crate) with a single `catch_up(&mut self)` method.
2. A `SharedProjectionCatchUp<P>` wrapper that holds an `Arc<Mutex<ProjectionRunner<P>>>` and delegates `catch_up` through the shared mutex.
3. A `projection_catch_ups: Arc<RwLock<ProjectionCatchUpList>>` field on `AggregateStore` that stores these wrappers.

Both the typed projection map (for `projection::<P>()`) and the catch-up list share the same `Arc<Mutex<ProjectionRunner<P>>>`, so catch-up state is consistent between the two access paths.

This mirrors the existing `ProcessManagerCatchUp` pattern used by process managers.

### Projection storage change: `Mutex` -> `Arc<Mutex>`

The projection map's values changed from `Box<Mutex<ProjectionRunner<P>>>` to `Box<Arc<Mutex<ProjectionRunner<P>>>>` (erased as `dyn Any`). The `projection()` and `rebuild_projection()` methods were updated to downcast to `Arc<Mutex<...>>` instead of `Mutex<...>`. All existing projection tests pass without changes.

### Actor-aware injection

When a live actor holds the exclusive flock on a stream's `app.jsonl`, opening a second `EventWriter` would fail. The implementation checks the handle cache for a live actor and routes through `inject_via_actor` (from Ticket 1) when one exists. For streams without a live actor, a temporary `EventWriter` is opened directly.

## Acceptance Criteria

- [x] AC 1: `inject_event` returns `Ok(())` and the event appears in the stream's JSONL - Test `inject_event_appends_to_stream` verifies this by reading the JSONL file after injection.
- [x] AC 2: Projections reflect injected events after `inject_event` returns - Test `inject_event_projections_reflect_event` verifies the EventCounter projection sees the injected event.
- [x] AC 3: Duplicate `event.id` is deduplicated (second call is no-op) - Test `inject_event_dedup_by_id` verifies only one event line appears after two injections with the same ID.
- [x] AC 4: Events with `event.id = None` are never deduplicated - Test `inject_event_no_dedup_for_none_id` verifies both events are written.
- [x] AC 5: `InjectOptions::default()` has `run_process_managers: false` - Test `inject_options_default_does_not_run_process_managers` and the doc-test verify this.
- [x] AC 6: `InjectOptions { run_process_managers: true }` triggers process managers - Test `inject_event_with_process_managers` verifies the ForwardSaga dispatches to the Receiver aggregate.
- [x] AC 7: Injecting into a stream with a live actor works without deadlock - Test `inject_event_with_live_actor` spawns an actor, injects an event, and verifies the actor's state reflects it.
- [x] AC 8: Injecting into a new stream creates the directory - Test `inject_event_creates_new_stream` verifies directory creation and state recovery via a fresh actor.
- [x] AC 9: `InjectOptions` and `inject_event` are re-exported from `lib.rs` with doc comments - `InjectOptions` is re-exported in `lib.rs`; `inject_event` is a public method on `AggregateStore` (already re-exported). Both have full doc comments.
- [x] AC 10: `cargo test` passes with no new failures - 103 unit tests + 8 doc-tests pass (up from 95 + 7).

## Test Results

- Lint: PASS (`cargo clippy -- -D warnings` - zero warnings)
- Tests: PASS (103 unit tests + 8 doc-tests, all passing)
- Build: PASS (`cargo build` - zero warnings)
- Format: PASS (`cargo fmt --check`)
- New tests added:
  - `src/store.rs::tests::inject_event_appends_to_stream`
  - `src/store.rs::tests::inject_event_projections_reflect_event`
  - `src/store.rs::tests::inject_event_dedup_by_id`
  - `src/store.rs::tests::inject_event_no_dedup_for_none_id`
  - `src/store.rs::tests::inject_options_default_does_not_run_process_managers`
  - `src/store.rs::tests::inject_event_with_process_managers`
  - `src/store.rs::tests::inject_event_with_live_actor`
  - `src/store.rs::tests::inject_event_creates_new_stream`

## Concerns / Blockers

- The `ProjectionCatchUpFn` trait and `SharedProjectionCatchUp` wrapper are private to the crate but represent new infrastructure beyond what was explicitly listed in the ticket scope. This was necessary to satisfy AC 2 (projections must reflect injected events). The alternative would have been to add a `ProjectionCatchUp` trait to `src/projection.rs`, but that file was not in the ticket's scope.
- The `#[allow(dead_code)]` attributes on `ActorMessage::Inject` and `inject_via_actor` (from Ticket 1) should be removed in a future cleanup pass, since `inject_event` now exercises both code paths. However, since those attributes are in `src/actor.rs` (not in this ticket's scope), they are left as-is.
- The `event.clone()` in the actor injection path is necessary because we need to keep the event alive for the `event_id` registration step. If the event is large, this could be optimized by extracting the ID before the clone, but this is a micro-optimization not warranted at this stage.
