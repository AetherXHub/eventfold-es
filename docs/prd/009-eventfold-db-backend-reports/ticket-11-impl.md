# Implementation Report: Ticket 11 -- src/store.rs: rewrite to AggregateStoreBuilder with endpoint + base_dir

**Ticket:** 11 - src/store.rs: rewrite to AggregateStoreBuilder with endpoint + base_dir
**Date:** 2026-02-27 15:30
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/store.rs` - Complete rewrite: replaced old file-based AggregateStore with gRPC-backed AggregateStoreBuilder pattern. Removed list_streams, read_events, and all eventfold JSONL dependencies.
- `src/lib.rs` - Uncommented `mod store;` and added `pub use store::{AggregateStore, AggregateStoreBuilder, InjectOptions};` re-exports.
- `examples/counter.rs` - Updated to use `AggregateStoreBuilder::new().endpoint(...).base_dir(...)` API and new `Projection::apply(&mut self, event: &StoredEvent)` signature.
- `Cargo.toml` - Changed `autoexamples = false` to `autoexamples = true` so examples compile as a build-time check.
- `src/client.rs` - Added `#[cfg(test)] pub(crate) fn from_inner()` constructor for test-only EsClient creation without a live server.
- `src/actor.rs` - Added `#[cfg(test)] pub(crate) fn from_sender()` constructor for test-only AggregateHandle creation without spawning an actor task.

## Implementation Notes

- **AggregateStoreBuilder pattern**: The builder collects configuration (endpoint URL, base_dir, projection/PM/aggregate type registrations, idle timeout) and defers all initialization to `open()`. `open()` calls `EsClient::connect(endpoint)` to establish the gRPC channel, then initializes all registered projections and process managers.

- **Handle cache key changed**: The cache is now keyed by `(TypeId, String)` instead of `(String, String)`. `TypeId::of::<A>()` is more robust than `A::AGGREGATE_TYPE` for distinguishing aggregate types in the generic context.

- **Projection catch-up is now async**: Since `ProjectionRunner::catch_up()` is async (uses gRPC subscribe), the store's `projection::<P>()` method is now `async`. This is a breaking change from the old synchronous API. The projection map uses `tokio::sync::Mutex` (not `std::sync::Mutex`) to safely hold the lock across `.await` points.

- **Process manager dispatch**: The `AggregateDispatcher` trait and `TypedDispatcher<A>` struct are defined within `store.rs` (not in `process_manager.rs`) since they are store-specific routing concerns that depend on `AggregateStore::get`.

- **inject_event signature change**: Takes `ProposedEventData` instead of `eventfold::Event`, matching the gRPC backend. Routes through `client.append(stream_id, Any, [proposed])`.

- **Test-only constructors**: Added `#[cfg(test)]` helpers on `EsClient::from_inner()` and `AggregateHandle::from_sender()` to enable unit tests that exercise cache logic without a live gRPC server. These are minimal additions to `src/client.rs` and `src/actor.rs` (outside the stated scope but necessary for testing).

## Acceptance Criteria
- [x] AC 1: AggregateStoreBuilder has methods: new(), endpoint(url), base_dir(path), aggregate_type::<A>(), projection::<P>(), process_manager::<PM>(), idle_timeout(Duration), async open() -> Result<AggregateStore, tonic::transport::Error>
- [x] AC 2: AggregateStore holds client: EsClient, base_dir: PathBuf, handle cache, projection map, PM list, dispatcher map, idle_timeout; no StreamLayout field
- [x] AC 3: AggregateStore::get::<A>(id) derives stream_id via stream_uuid, spawns actor via spawn_actor_with_config(id, client.clone(), base_dir, config)
- [x] AC 4: AggregateStore::inject_event::<A>(id, proposed, opts) calls client.append(stream_id, Any, [proposed]); if opts.run_process_managers triggers run_process_managers()
- [x] AC 5: AggregateStore::projection::<P>() triggers runner.catch_up() and returns cloned state -- now async
- [x] AC 6: list_streams and read_events methods removed
- [x] AC 7: examples/counter.rs updated to use AggregateStoreBuilder and new Projection::apply signature
- [x] AC 8: Test: AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await returns Err when no server is listening
- [x] AC 9: Test: AggregateStore::get::<Counter>("c-1") called twice returns handles where both is_alive() (cached fast path)
- [x] AC 10: Quality gates pass

## Test Results
- Lint (fmt): PASS
- Lint (clippy): PASS (non-test); test clippy has 4 pre-existing `result_large_err` warnings in `projection.rs` and `process_manager.rs` test code (out of scope)
- Tests: PASS -- 93 unit tests + 5 doctests, 0 failures
- Build: PASS -- zero warnings
- New tests added:
  - `src/store.rs::tests::builder_connect_returns_err_when_no_server`
  - `src/store.rs::tests::get_twice_returns_cached_alive_handles`

## Concerns / Blockers
- **Out-of-scope file modifications**: Added `#[cfg(test)]` constructors to `src/client.rs` (`from_inner`) and `src/actor.rs` (`from_sender`). These are test-only, zero-impact changes needed for the AC9 test. The alternative would have been to require a live gRPC server for unit tests, which is impractical.
- **Pre-existing clippy warnings**: `cargo clippy --tests -- -D warnings` fails due to `result_large_err` warnings in `projection.rs` and `process_manager.rs` test helper functions. These are pre-existing and outside this ticket's scope. They pass with `-A clippy::result_large_err`.
- **`open()` panics on projection/PM init failure**: Since `tonic::transport::Error` is opaque (cannot be constructed from `io::Error`), the `open()` method currently panics if projection or process manager initialization fails (which only happens on filesystem I/O errors reading checkpoint files). A future improvement could change the return type to a custom error enum that can represent both transport and I/O errors.
- **`autoexamples` re-enabled**: Changed from `false` to `true` in `Cargo.toml`. The counter example compiles but requires a running `eventfold-db` server to execute.
