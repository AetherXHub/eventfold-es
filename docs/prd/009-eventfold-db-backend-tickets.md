# Tickets for PRD 009: eventfold-db gRPC Backend

**Source PRD:** docs/prd/009-eventfold-db-backend.md
**Created:** 2026-02-27
**Total Tickets:** 12
**Estimated Total Complexity:** 28 (S=1, M=2, L=3)

> **Version bump note:** This is a semver-major migration (0.2 -> 0.3). All
> tests from prior PRDs that reference `eventfold::Event`, `reducer()`,
> `to_eventfold_event()`, `spawn_actor(stream_dir)`, `list_streams`, or
> `read_events` must be removed or rewritten as part of this work. The
> `eventfold` crate dependency is fully dropped.

---

### Ticket 1: Cargo.toml, build.rs, and proto codegen

**Description:**
Wire up the `tonic`/`prost` gRPC toolchain. Update `Cargo.toml` to add all
new runtime and build-time dependencies and remove `eventfold`. Create
`build.rs` that compiles `../eventfold-db/proto/eventfold.proto` so the
generated gRPC types are available to every subsequent ticket.

**Scope:**
- Modify: `Cargo.toml`
- Create: `build.rs`

**Acceptance Criteria:**
- [ ] `Cargo.toml` removes `eventfold = "0.2.0"` and adds: `tonic = "0.13"`, `prost = "0.13"`, `bytes = "1"`, `tokio-stream = "0.1"`, `uuid = { version = "1", features = ["v4", "v5"] }`
- [ ] `Cargo.toml` adds `[build-dependencies]` section with `tonic-build = "0.13"`
- [ ] `build.rs` calls `tonic_build::compile_protos("../eventfold-db/proto/eventfold.proto")` and propagates errors
- [ ] `build.rs` emits `cargo:rerun-if-changed=../eventfold-db/proto/eventfold.proto` so Cargo re-runs codegen on proto changes
- [ ] Test: `cargo build` with no other source changes compiles cleanly (proto codegen succeeds, generated module accessible via `include!(concat!(env!("OUT_DIR"), "/eventfold.rs"))`)
- [ ] `cargo clippy -- -D warnings` and `cargo fmt --check` pass

**Dependencies:** None
**Complexity:** S
**Maps to PRD AC:** AC 12

---

### Ticket 2: `src/event.rs` — StoredEvent, EventMetadata, encode/decode helpers, stream_uuid

**Description:**
Create the `src/event.rs` module containing all shared types and pure
functions for event encoding and decoding. This is the foundational data
layer that the gRPC client, actor, projection, and process manager all depend
on. No network I/O occurs in this module.

**Scope:**
- Create: `src/event.rs`
- Modify: `src/lib.rs` (add `mod event; pub use event::StoredEvent;`)

**Acceptance Criteria:**
- [ ] `StoredEvent` struct has fields: `event_id: Uuid`, `stream_id: Uuid`, `aggregate_type: String`, `instance_id: String`, `stream_version: u64`, `global_position: u64`, `event_type: String`, `payload: serde_json::Value`, `metadata: serde_json::Value`, `recorded_at: u64`; derives `Debug, Clone`
- [ ] `EventMetadata` struct has fields: `aggregate_type: String`, `instance_id: String`, `actor: Option<String>`, `correlation_id: Option<String>`, `source_device: Option<String>`; derives `Serialize, Deserialize`; uses `#[serde(skip_serializing_if = "Option::is_none")]` on optional fields
- [ ] `STREAM_NAMESPACE: Uuid` constant matches the exact bytes from the PRD: `[0x9a, 0x1e, 0x7c, 0x3b, 0x4d, 0x2f, 0x4a, 0x8e, 0xb5, 0x6c, 0x1f, 0x3d, 0x7e, 0x9a, 0x0b, 0xc4]`
- [ ] `pub fn stream_uuid(aggregate_type: &str, instance_id: &str) -> Uuid` uses `Uuid::new_v5(&STREAM_NAMESPACE, ...)` with `"{aggregate_type}/{instance_id}"` as input
- [ ] `pub fn encode_domain_event<A: Aggregate>(event: &A::DomainEvent, ctx: &CommandContext, aggregate_type: &str, instance_id: &str) -> serde_json::Result<ProposedEventData>` where `ProposedEventData { event_id: Uuid, event_type: String, payload: serde_json::Value, metadata: EventMetadata }` serializes the adjacently-tagged domain event, extracts `type` and `data` fields, populates `EventMetadata`, and generates a UUID v4 `event_id`
- [ ] `pub fn decode_stored_event(recorded: &RecordedEvent) -> Option<StoredEvent>` parses `recorded.metadata` as JSON into `EventMetadata`, returns `None` if `aggregate_type` or `instance_id` are absent or metadata is unparseable, otherwise returns `Some(StoredEvent)` with all fields populated from `RecordedEvent` fields and decoded metadata
- [ ] Test: `stream_uuid("counter", "c-1")` called twice returns the same `Uuid` value (determinism across calls)
- [ ] Test: `stream_uuid("counter", "c-1")` != `stream_uuid("counter", "c-2")` and != `stream_uuid("order", "c-1")` (uniqueness)
- [ ] Test: `encode_domain_event` with a `CounterEvent::Incremented` produces `event_type == "Incremented"`, `payload` is a JSON object or null, `metadata.aggregate_type == "counter"`, `metadata.instance_id == "c-1"`, and `event_id` is a valid UUID v4
- [ ] Test: `encode_domain_event` with `CommandContext::with_actor("u1").with_correlation_id("c1").with_source_device("d1")` populates all three optional `EventMetadata` fields
- [ ] Test: `decode_stored_event` on a `RecordedEvent` with well-formed metadata JSON returns `Some(StoredEvent)` with `aggregate_type` and `instance_id` matching what was encoded
- [ ] Test: `decode_stored_event` on a `RecordedEvent` whose `metadata` bytes are `b"{}"` (missing `aggregate_type`) returns `None`
- [ ] Test: `decode_stored_event` on a `RecordedEvent` whose `metadata` bytes are invalid UTF-8/JSON returns `None`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Ticket 1 (proto codegen must be available for `RecordedEvent` type)
**Complexity:** M
**Maps to PRD AC:** AC 3, AC 10, AC 11

---

### Ticket 3: `src/error.rs` — add Transport and WrongExpectedVersion variants

**Description:**
Extend `ExecuteError` with two new variants required by the gRPC actor loop:
`Transport(tonic::Status)` for network-level errors, and rename/repurpose
`Conflict` to `WrongExpectedVersion` for the optimistic concurrency error path.
Also update `StateError` to accept `Transport` errors.

**Scope:**
- Modify: `src/error.rs`

**Acceptance Criteria:**
- [ ] `ExecuteError::WrongExpectedVersion` variant exists (replaces `Conflict`; same `#[error("optimistic concurrency conflict: retries exhausted")]` message is acceptable, or update to match the PRD name)
- [ ] `ExecuteError::Transport(tonic::Status)` variant exists with `#[error("gRPC transport error: {0}")]`
- [ ] `StateError::Transport(tonic::Status)` variant exists with `#[error("gRPC transport error: {0}")]`
- [ ] All existing `ExecuteError` and `StateError` tests updated to reference new variant names where needed; no tests removed for non-conflicting variants (`Domain`, `Io`, `ActorGone` remain)
- [ ] Test: constructing `ExecuteError::WrongExpectedVersion` and calling `.to_string()` returns a non-empty string
- [ ] Test: constructing `ExecuteError::Transport(tonic::Status::internal("boom"))` and calling `.to_string()` contains `"boom"` or `"gRPC"`
- [ ] `Send + Sync` compile-time assertions still pass for `ExecuteError<TestDomainError>` and `StateError`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Ticket 1 (tonic crate must be in Cargo.toml)
**Complexity:** S
**Maps to PRD AC:** AC 9, AC 12

---

### Ticket 4: `src/client.rs` — EsClient gRPC wrapper

**Description:**
Create `src/client.rs` as a thin, typed wrapper around the tonic-generated
`EventStoreClient<Channel>`. Expose three async methods — `append`,
`read_stream`, and `subscribe_all_from` — with ergonomic Rust types so that
the actor and projection code never import tonic internals directly.

**Scope:**
- Create: `src/client.rs`
- Modify: `src/lib.rs` (add `mod client; pub use client::EsClient;`)

**Acceptance Criteria:**
- [ ] `pub struct EsClient` wraps `eventfold::EventStoreClient<Channel>` (where `eventfold` is the prost-generated module alias); derives `Clone`
- [ ] `EsClient::connect(endpoint: &str) -> Result<Self, tonic::transport::Error>` creates a channel and returns `Ok(client)` on success
- [ ] `EsClient::append(&mut self, stream_id: Uuid, expected: ExpectedVersionArg, events: Vec<ProposedEventData>) -> Result<AppendResponse, tonic::Status>` converts `ProposedEventData` into proto `ProposedEvent` messages (serializing `payload` and `metadata` as JSON bytes) and calls `Append` RPC
- [ ] `EsClient::read_stream(&mut self, stream_id: Uuid, from_version: u64, max_count: u64) -> Result<Vec<RecordedEvent>, tonic::Status>` calls `ReadStream` RPC and returns the `events` vec
- [ ] `EsClient::subscribe_all_from(&mut self, from_position: u64) -> Result<impl Stream<Item = Result<SubscribeResponse, tonic::Status>>, tonic::Status>` calls `SubscribeAll` RPC and returns the server-streaming response
- [ ] `ExpectedVersionArg` is a local enum with variants `Any`, `NoStream`, `Exact(u64)` that converts to the proto `ExpectedVersion` oneof
- [ ] Test: unit test that constructs a `ProposedEventData` with known payload/metadata, passes it through `append`'s internal conversion logic (extract the serialization step into a `fn to_proto_event(data: &ProposedEventData) -> proto::ProposedEvent` helper), and asserts that `payload` bytes deserialize back to the original JSON and `metadata` bytes deserialize back to `EventMetadata`
- [ ] Test: `ExpectedVersionArg::Any` converts to a proto `ExpectedVersion` with `kind = Some(any)`, `ExpectedVersionArg::Exact(5)` converts to `kind = Some(exact(5))`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 1, 2 (proto types, `ProposedEventData`, `EventMetadata`)
**Complexity:** M
**Maps to PRD AC:** AC 1, AC 8

---

### Ticket 5: `src/snapshot.rs` — local file-based snapshot module

**Description:**
Create `src/snapshot.rs` with atomic read/write of `Snapshot<A>` to
`<base_dir>/snapshots/<type>/<id>/snapshot.json`. This is the local
persistence layer for aggregate state that replaces the old `eventfold::View`.

**Scope:**
- Create: `src/snapshot.rs`

**Acceptance Criteria:**
- [ ] `pub struct Snapshot<A>` has fields `state: A` and `stream_version: u64`; derives `Serialize, Deserialize` where `A: Serialize + DeserializeOwned`
- [ ] `pub fn snapshot_path(base_dir: &Path, aggregate_type: &str, instance_id: &str) -> PathBuf` returns `<base_dir>/snapshots/<aggregate_type>/<instance_id>/snapshot.json`
- [ ] `pub fn save_snapshot<A: Aggregate>(base_dir: &Path, instance_id: &str, snapshot: &Snapshot<A>) -> io::Result<()>` writes atomically via temp-rename (write to `snapshot.json.tmp`, then `fs::rename`)
- [ ] `pub fn load_snapshot<A: Aggregate>(base_dir: &Path, instance_id: &str) -> io::Result<Option<Snapshot<A>>>` returns `Ok(None)` if file does not exist; on deserialization failure, logs a warning and returns `Ok(None)` (cache miss — actor will replay from stream)
- [ ] Test: `save_snapshot` then `load_snapshot` roundtrips: save `Snapshot { state: Counter { value: 7 }, stream_version: 3 }`, reload, assert `state.value == 7` and `stream_version == 3`
- [ ] Test: `load_snapshot` on a non-existent path returns `Ok(None)`
- [ ] Test: `load_snapshot` on a corrupt JSON file returns `Ok(None)` (not `Err`)
- [ ] Test: `save_snapshot` is atomic — if the process were interrupted after writing `snapshot.json.tmp` but before rename, no corrupt `snapshot.json` exists (verify via inspecting that temp file is in the same directory so rename is atomic on POSIX)
- [ ] Test: `snapshot_path` returns the exact expected path `<base_dir>/snapshots/counter/c-1/snapshot.json`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Ticket 2 (`Aggregate` trait still available; `Counter` test fixture still in `aggregate.rs`)
**Complexity:** S
**Maps to PRD AC:** AC 4, AC 12

---

### Ticket 6: `src/storage.rs` — narrow to snapshot directory helpers only

**Description:**
Strip `StreamLayout` down to only the paths still needed after the gRPC
migration: `base_dir()`, `snapshots_dir()`, `projections_dir()`, and
`process_managers_dir()`. Remove JSONL-stream paths (`stream_dir`,
`views_dir`, `ensure_stream`, `list_aggregate_types`, `list_streams`, the
registry file) since they are all JSONL-backend concerns. Update `src/lib.rs`
to remove the re-export of `StreamLayout` (it becomes `pub(crate)`).

**Scope:**
- Modify: `src/storage.rs`
- Modify: `src/lib.rs` (remove `pub use storage::StreamLayout`)

**Acceptance Criteria:**
- [ ] `StreamLayout` retains only: `new()`, `base_dir()`, `projections_dir()`, `process_managers_dir()`, and `snapshots_dir()` (new: returns `<base_dir>/snapshots`)
- [ ] All JSONL-era methods removed: `stream_dir`, `views_dir`, `ensure_stream`, `list_streams`, `list_aggregate_types`, and the `meta_dir` / registry file logic
- [ ] `StreamLayout` visibility changed to `pub(crate)` (no longer part of the public API)
- [ ] All tests in `src/storage.rs` updated to only test the remaining methods; old JSONL-related tests removed
- [ ] Test: `layout.snapshots_dir()` returns `<base_dir>/snapshots`
- [ ] Test: `layout.projections_dir()` returns `<base_dir>/projections`
- [ ] Test: `layout.process_managers_dir()` returns `<base_dir>/process_managers`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 2, 5 (snapshot path helpers are now in `snapshot.rs`; storage no longer needs stream paths)
**Complexity:** S
**Maps to PRD AC:** AC 12

---

### Ticket 7: `src/aggregate.rs` — remove JSONL bridge functions; update tests

**Description:**
Remove `reducer()`, `reduce()`, and `to_eventfold_event()` from
`src/aggregate.rs` since the gRPC backend no longer uses `eventfold::Event`.
Remove the corresponding tests for those functions. The `Aggregate` trait
itself is unchanged. Update `src/lib.rs` to remove those re-exports.

**Scope:**
- Modify: `src/aggregate.rs`
- Modify: `src/lib.rs` (remove `pub use aggregate::{reducer, to_eventfold_event}`)

**Acceptance Criteria:**
- [ ] `reducer()`, `reduce<A>()`, and `to_eventfold_event()` are deleted from `src/aggregate.rs`
- [ ] All imports of `eventfold` crate removed from `src/aggregate.rs`
- [ ] `src/lib.rs` no longer exports `reducer` or `to_eventfold_event`
- [ ] All tests in `src/aggregate.rs` that reference `reducer`, `to_eventfold_event`, `eventfold::Event`, or `eventfold::ReduceFn` are removed
- [ ] The remaining `Aggregate` trait tests (`handle_increment`, `handle_decrement_*`, `handle_add`, `apply_*`, `handle_then_apply_roundtrip`) are preserved and still pass
- [ ] The `test_fixtures` module (`Counter`, `CounterCommand`, `CounterError`, `CounterEvent`) is retained intact as `pub(crate)` for use by other test modules
- [ ] Test: `Counter::handle(Increment)` still returns `vec![CounterEvent::Incremented]`
- [ ] Test: `Counter::apply(Incremented)` still returns `Counter { value: 1 }`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Ticket 2 (`encode_domain_event` in `event.rs` now covers the serialization bridge)
**Complexity:** S
**Maps to PRD AC:** AC 12

---

### Ticket 8: `src/actor.rs` — full rewrite to gRPC-backed actor loop

**Description:**
Rewrite `src/actor.rs` to replace all `eventfold::{EventWriter, EventReader,
View}` usage with the gRPC client + snapshot module. The actor holds
`(state: A, stream_version: u64, stream_id: Uuid)`, loads from snapshot on
spawn, catches up via `client.read_stream`, executes commands via
`client.append(Exact(version))` with up to 3 retries on
`FAILED_PRECONDITION`, and saves a snapshot on idle shutdown.

**Scope:**
- Modify: `src/actor.rs` (full rewrite)

**Acceptance Criteria:**
- [ ] `ActorMessage<A>::Inject` variant changed from `event: eventfold::Event` to `proposed: ProposedEventData`; reply type stays `oneshot::Sender<Result<(), tonic::Status>>`
- [ ] `AggregateHandle<A>` no longer holds an `EventReader`; remove `reader()` accessor method; `is_alive()` and `execute()`, `state()` remain
- [ ] `spawn_actor_with_config<A>` signature changes to `(instance_id: &str, client: EsClient, base_dir: &Path, config: ActorConfig) -> AggregateHandle<A>` (no more `stream_dir`)
- [ ] Public `spawn_actor(stream_dir)` function removed from public API (delete or make `pub(crate)`)
- [ ] Actor loop on spawn: loads `Snapshot<A>` via `load_snapshot`, derives `stream_id` via `stream_uuid`, then calls `client.read_stream(stream_id, snapshot.stream_version + 1, u64::MAX)` and folds each event's payload through `A::apply`
- [ ] Actor loop on `Execute`: encodes events via `encode_domain_event`, calls `client.append(stream_id, Exact(version), events)`, on `FAILED_PRECONDITION` re-reads from `version + 1` and retries up to `MAX_RETRIES = 3`; on 3rd failure returns `Err(ExecuteError::WrongExpectedVersion)`
- [ ] Actor loop on idle timeout or `Shutdown`: saves snapshot via `save_snapshot` before exiting
- [ ] Test: construct actor with a mock/stub that returns 3 successive `FAILED_PRECONDITION` errors — `execute()` returns `Err(ExecuteError::WrongExpectedVersion)` (Implementer: use a channel-based test double or `tokio::sync::watch` to inject canned gRPC responses)
- [ ] Test: construct actor, execute `Increment` once, send `Shutdown`, then call `load_snapshot` on the base_dir — snapshot exists with `stream_version == 0` and `state.value == 1` (verifies snapshot-on-shutdown)
- [ ] Test: construct actor with a pre-existing snapshot at `stream_version = 2, state.value = 2`, mock `read_stream` returns one more `Incremented` event at version 3 — after spawn, `state().await` returns `Counter { value: 3 }`
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 1–7 (needs `EsClient`, `encode_domain_event`, `stream_uuid`, `Snapshot`, `StoredEvent`, updated `error.rs`)
**Complexity:** L
**Maps to PRD AC:** AC 2, AC 4, AC 5, AC 8, AC 9

---

### Ticket 9: `src/projection.rs` — rewrite to gRPC SubscribeAll cursor

**Description:**
Rewrite `src/projection.rs` to replace per-stream byte-offset cursors with a
single `GlobalCursor { position: u64 }` backed by `client.subscribe_all_from`.
Simplify the `Projection` trait by removing `subscriptions()`. Update the
`ProjectionCheckpoint` structure accordingly. The `ProjectionRunner::catch_up`
now consumes the `SubscribeAll` stream until `CaughtUp`, decodes each
`RecordedEvent` into `StoredEvent` (skipping non-ES events), calls
`projection.apply(&stored_event)`, and saves the updated checkpoint.

**Scope:**
- Modify: `src/projection.rs` (full rewrite of trait + runner)

**Acceptance Criteria:**
- [ ] `Projection` trait loses `subscriptions()` method; `apply` signature changes to `fn apply(&mut self, event: &StoredEvent)`
- [ ] `ProjectionCheckpoint<P>` replaces `cursors: HashMap<...>` with `last_global_position: u64`; serializes to `{ "state": ..., "last_global_position": N }`
- [ ] `ProjectionRunner<P>` holds `client: EsClient` and `checkpoint_dir: PathBuf` (no more `StreamLayout`)
- [ ] `ProjectionRunner::new(client: EsClient, checkpoint_dir: PathBuf)` loads checkpoint from disk or defaults to position 0
- [ ] `ProjectionRunner::catch_up(&mut self)` calls `client.subscribe_all_from(last_global_position)`, reads until `CaughtUp`, for each `RecordedEvent` calls `decode_stored_event` (skips `None`), calls `projection.apply(&stored_event)`, updates `last_global_position`, then saves checkpoint
- [ ] Events with missing or unparseable metadata (non-ES events) are silently skipped (no error, no panic)
- [ ] Test: `catch_up` on a fresh checkpoint with position 0 and a mock stream returning two `RecordedEvent`s (with valid ES metadata) followed by `CaughtUp` results in `projection.count == 2` and saved `last_global_position == max(global_position of the two events)`
- [ ] Test: a second `catch_up` with `from_position = last_global_position` and a mock stream returning only `CaughtUp` leaves count unchanged (idempotency)
- [ ] Test: a `RecordedEvent` with `metadata = b"{}"` (no `aggregate_type`) is skipped; count stays at 0
- [ ] Test: `save_checkpoint` followed by `load_checkpoint` roundtrips `last_global_position` and projection state correctly
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 2, 4 (`StoredEvent`, `decode_stored_event`, `EsClient`, `subscribe_all_from`)
**Complexity:** M
**Maps to PRD AC:** AC 6, AC 10, AC 12

---

### Ticket 10: `src/process_manager.rs` — rewrite to gRPC SubscribeAll cursor

**Description:**
Mirror the projection rewrite for process managers: remove `subscriptions()`,
change `react` to take `&StoredEvent`, replace per-stream byte-offset cursors
with `last_global_position: u64`, and update `ProcessManagerRunner::catch_up`
to consume `SubscribeAll` until `CaughtUp`.

**Scope:**
- Modify: `src/process_manager.rs` (full rewrite of trait + runner)

**Acceptance Criteria:**
- [ ] `ProcessManager` trait loses `subscriptions()` method; `react` signature changes to `fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>`
- [ ] `ProcessManagerCheckpoint<PM>` uses `last_global_position: u64` (same structure as `ProjectionCheckpoint`)
- [ ] `ProcessManagerRunner<PM>` holds `client: EsClient` and `checkpoint_dir: PathBuf`
- [ ] `catch_up` consumes `SubscribeAll` stream until `CaughtUp`, calls `decode_stored_event` (skips `None`), calls `pm.react(&stored_event)`, collects envelopes; does NOT save checkpoint (caller calls `save` after dispatch)
- [ ] `save` persists `last_global_position` and PM state
- [ ] `ProcessManagerCatchUp` trait object interface updated to match new signatures
- [ ] Test: `catch_up` on mock stream with one valid ES event returns `vec![CommandEnvelope { ... }]` of length 1 and updates `last_global_position`
- [ ] Test: a second `catch_up` with saved checkpoint at `last_global_position` and mock stream returning only `CaughtUp` returns empty `Vec`
- [ ] Test: non-ES events (missing metadata) are skipped; `catch_up` returns empty `Vec`
- [ ] Test: `ProcessManagerCheckpoint` serialization roundtrip: `{ state: EchoSaga { events_seen: 3 }, last_global_position: 42 }` serializes to JSON and deserializes back correctly
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 2, 4, 9 (same pattern as projection; `StoredEvent`, `EsClient`, gRPC types)
**Complexity:** M
**Maps to PRD AC:** AC 7, AC 12

---

### Ticket 11: `src/store.rs` — rewrite to AggregateStoreBuilder with endpoint + base_dir

**Description:**
Rewrite `src/store.rs` so that `AggregateStore` holds an `EsClient` instead of
a `StreamLayout`, is opened via `AggregateStoreBuilder::new().endpoint(...).base_dir(...).open().await`,
and routes `inject_event` through `client.append(Any)`. Remove `list_streams`,
`read_events`, `AggregateStore::open(path)`, `AggregateStore::list`, and the
`StreamLayout` field. Update `src/lib.rs` to remove dropped re-exports and add
new ones (`StoredEvent`, `EsClient`). Update `examples/counter.rs` to use the
new builder API and the new `Projection::apply` signature.

**Scope:**
- Modify: `src/store.rs` (full rewrite)
- Modify: `src/lib.rs` (update re-exports)
- Modify: `examples/counter.rs` (update to new API)

**Acceptance Criteria:**
- [ ] `AggregateStoreBuilder` has methods `new() -> Self`, `endpoint(url: &str) -> Self`, `base_dir(path: impl AsRef<Path>) -> Self`, `aggregate_type::<A: Aggregate>() -> Self`, `projection::<P: Projection>() -> Self`, `process_manager::<PM: ProcessManager>() -> Self`, `idle_timeout(Duration) -> Self`, and `async open() -> Result<AggregateStore, tonic::transport::Error>`
- [ ] `AggregateStore` holds `client: EsClient`, `base_dir: PathBuf`, handle cache, projection map, PM list, dispatcher map, `idle_timeout`; no `StreamLayout` field
- [ ] `AggregateStore::get::<A>(id)` derives `stream_id` via `stream_uuid`, spawns actor via `spawn_actor_with_config(id, client.clone(), base_dir, config)`
- [ ] `AggregateStore::inject_event::<A>(id, proposed, opts)` calls `client.append(stream_id, Any, [proposed])`; if `opts.run_process_managers` is true, triggers `run_process_managers()`
- [ ] `AggregateStore::projection::<P>()` triggers `runner.catch_up()` and returns cloned state
- [ ] `list_streams` and `read_events` methods removed (they do not exist on `AggregateStore`)
- [ ] `examples/counter.rs` updated to use `AggregateStoreBuilder::new().endpoint(...).base_dir(...).open().await` and the new `Projection::apply(&StoredEvent)` signature
- [ ] Test: `AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await` returns `Err` when no server is listening (Implementer: use a random high port that is guaranteed unbound, e.g. port 1 or check for `ConnectionRefused`)
- [ ] Test: `AggregateStore::get::<Counter>("c-1")` called twice returns handles where both `is_alive()` (cached fast path exercised)
- [ ] Quality gates pass (build, lint, fmt, tests)

**Dependencies:** Tickets 1–10 (all modules must be available before the store can wire them together)
**Complexity:** L
**Maps to PRD AC:** AC 1, AC 2, AC 5, AC 6, AC 7, AC 8, AC 12

---

### Ticket 12: Verification & Integration Test

**Description:**
Run the full PRD acceptance criteria checklist end-to-end against a live
`eventfold-db` server. Verify all tickets integrate correctly as a cohesive
feature: builder API, aggregate execution with optimistic concurrency, snapshot
round-trip, concurrent stores, projection catch-up, process manager catch-up,
inject_event, and stream_uuid determinism. All tests must pass on a clean
checkout with `eventfold-db` running.

**Scope:**
- Modify: (no new production files; may add integration test helpers in `tests/` if needed)

**Acceptance Criteria:**
- [ ] AC 1: `AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await` returns `Ok(store)`; same builder with a non-listening endpoint returns `Err`
- [ ] AC 2: three `handle.execute(Increment, ctx).await` calls on `Counter("c-1")` leave `handle.state().await == Counter { value: 3 }` and `client.read_stream(stream_uuid("counter","c-1"), 0, 100)` returns exactly 3 events with `event_type == "Incremented"`
- [ ] AC 3: every `RecordedEvent` payload from the above has metadata bytes that deserialize to `EventMetadata` with non-empty `aggregate_type` and `instance_id`
- [ ] AC 4: after idle eviction (drop all handles + wait for timeout), `store.get::<Counter>("c-1")` returns a handle with state == prior state (snapshot round-trip)
- [ ] AC 5: two concurrent `AggregateStore` instances each `Increment` the same `Counter("c-1")` simultaneously — both `execute` calls return `Ok` and `read_stream` returns exactly 2 events
- [ ] AC 6: `ProjectionRunner::<EventCounter>::catch_up()` after two `Increment` events from two instances returns `count == 2`; second `catch_up` without new writes leaves count unchanged
- [ ] AC 7: `ProcessManagerRunner::<EchoSaga>::catch_up()` after one `Increment` returns a `Vec<CommandEnvelope>` of length 1; second `catch_up` returns empty
- [ ] AC 8: `store.inject_event::<Counter>("c-1", proposed, InjectOptions::default()).await` returns `Ok` and the event appears in `client.read_stream`
- [ ] AC 9: mocked/stubbed test (from Ticket 8) where 3 consecutive `FAILED_PRECONDITION` responses cause `execute` to return `Err(ExecuteError::WrongExpectedVersion)` — confirmed passing
- [ ] AC 10: every `StoredEvent` delivered to `EventCounter::apply` has `recorded_at > 0` and non-empty `aggregate_type`/`instance_id`
- [ ] AC 11: `stream_uuid("counter", "c-1")` returns the same value in two separate processes (verify by hardcoding the expected UUID derived from the spec namespace)
- [ ] AC 12: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass with no warnings
- [ ] No regressions in `src/command.rs` tests (CommandBus, CommandContext, CommandEnvelope)
- [ ] `examples/counter.rs` compiles and runs successfully with `eventfold-db` running

**Dependencies:** All previous tickets
**Complexity:** M

---

## AC Coverage Matrix

| PRD AC # | Description | Covered By Ticket(s) | Status |
|----------|-------------|----------------------|--------|
| 1 | `AggregateStoreBuilder::new().endpoint(...).open()` returns `Ok` when server is up; `Err` when not reachable | Ticket 4 (client connect), Ticket 11 (builder), Ticket 12 (integration) | Covered |
| 2 | Three `execute(Increment)` calls produce `Counter { value: 3 }` and 3 server-side `RecordedEvent`s | Ticket 8 (actor execute loop), Ticket 11 (store wiring), Ticket 12 | Covered |
| 3 | Every written `RecordedEvent` has metadata with `aggregate_type` and `instance_id` as JSON string fields | Ticket 2 (`EventMetadata`/`encode_domain_event`), Ticket 8 (actor uses encode), Ticket 12 | Covered |
| 4 | Snapshot round-trip: drop + re-get returns same state | Ticket 5 (snapshot module), Ticket 8 (save-on-shutdown + load-on-spawn), Ticket 12 | Covered |
| 5 | Two concurrent stores, same instance ID, both `execute` return `Ok`, 2 events total | Ticket 8 (OCC retry loop), Ticket 12 | Covered |
| 6 | `ProjectionRunner::catch_up` after 2 events from 2 instances → `count == 2`; idempotent second call | Ticket 9 (projection rewrite), Ticket 12 | Covered |
| 7 | `ProcessManagerRunner::catch_up` after 1 event → 1 envelope; second call → empty | Ticket 10 (PM rewrite), Ticket 12 | Covered |
| 8 | `inject_event` appends with `ExpectedVersion::Any` and event is readable | Ticket 4 (`EsClient::append`), Ticket 8 (`Inject` message), Ticket 11 (store `inject_event`), Ticket 12 | Covered |
| 9 | Three consecutive `FAILED_PRECONDITION` → `Err(ExecuteError::WrongExpectedVersion)` | Ticket 3 (error variant), Ticket 8 (retry loop), Ticket 12 | Covered |
| 10 | Every `StoredEvent` to `apply` has `recorded_at > 0` and non-empty `aggregate_type`/`instance_id` | Ticket 2 (`decode_stored_event`), Ticket 9 (projection applies `StoredEvent`), Ticket 12 | Covered |
| 11 | `stream_uuid("counter", "c-1")` is deterministic across processes | Ticket 2 (`stream_uuid` with fixed namespace), Ticket 12 | Covered |
| 12 | `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass | Every ticket (quality gates), Ticket 12 | Covered |
