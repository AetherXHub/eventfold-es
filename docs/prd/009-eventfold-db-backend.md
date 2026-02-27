# PRD 009: eventfold-db gRPC Backend

**Status:** IMPLEMENTED
**Created:** 2026-02-27
**Revised:** 2026-02-27
**Author:** PRD Writer Agent

---

## Problem Statement

`eventfold-es` is currently hard-coupled to the `eventfold` crate, which stores each
aggregate stream as a per-process JSONL file protected by an OS advisory flock. This
limits every `AggregateStore` to a single process on a single machine: two processes
cannot safely share one store, and there is no global event ordering across streams.
`eventfold-db` already exists in the same organisation as a purpose-built, single-node
event store exposing a gRPC API with global ordering, optimistic concurrency, push-based
subscriptions, and server-side idempotent dedup -- all of which are exactly what
multi-process, multi-machine deployments of `eventfold-es` need.

## Prerequisites

All upstream prerequisites are satisfied:

- **eventfold-db PRD 017** (Server-Assigned Timestamps): `recorded_at: u64` (Unix epoch
  milliseconds) is present on `RecordedEvent`. Landed in commit `a66c310`.
- **eventfold-db PRD 014** (ListStreams RPC): `ListStreams` RPC returns `StreamInfo` with
  `stream_id`, `event_count`, and `latest_version`. Landed in commit `a09a83f`.

## Goals

- Replace the `eventfold` JSONL storage backend with a tonic gRPC client that talks to
  a running `eventfold-db` server, eliminating per-process file locking.
- Preserve the `Aggregate` trait unchanged (no signature changes).
- Map `(aggregate_type, instance_id)` to `Uuid` stream IDs using deterministic UUID v5
  derivation. Encode `aggregate_type` and `instance_id` in event metadata so that
  projections and process managers can recover them without any local registry.
- Replace byte-offset cursors in `ProjectionRunner` and `ProcessManagerRunner` with
  `global_position`-based cursors backed by the `SubscribeAll` gRPC stream.
- Implement proper optimistic concurrency in the actor loop using
  `ExpectedVersion::Exact(stream_version)` and bounded retry on `WrongExpectedVersion`.
- Maintain local file-based snapshots so actors do not re-read an entire stream history
  on every spawn.
- Remove the JSONL-based `list_streams` and `read_events` from the public API. Tooling
  can use the `ListStreams` gRPC RPC directly via `EsClient` if needed.

### Breaking changes summary

| Change | Who is affected |
|--------|-----------------|
| `Projection::apply` signature simplified to `(&mut self, event: &StoredEvent)` | All `Projection` implementors |
| `ProcessManager::react` signature simplified to `(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>` | All `ProcessManager` implementors |
| `Projection::subscriptions` removed (filtering by `StoredEvent.aggregate_type` in body) | All `Projection` implementors |
| `ProcessManager::subscriptions` removed (same) | All `ProcessManager` implementors |
| `AggregateStore::open(path)` -> `AggregateStoreBuilder::new().endpoint(...).open()` | All store construction call sites |
| `AggregateHandle::reader()` removed | Callers accessing the underlying `EventReader` |
| `list_streams`, `read_events` removed | Callers using stream enumeration |
| `spawn_actor(stream_dir)` removed from public API | Direct actor spawners (if any) |
| `to_eventfold_event()`, `reducer()` removed from public API | Direct bridge function callers (if any) |
| `StreamLayout` removed or narrowed to snapshot paths | Callers using `stream_dir()`, `views_dir()` |

This constitutes a semver-major bump (0.2 -> 0.3).

## Non-Goals

- Implementing TLS client configuration; the initial client uses plaintext only.
- Adding cross-process rate limiting or retry backoff beyond a simple bounded-retry loop.
- Migrating existing on-disk JSONL event data; this requires a fresh `eventfold-db` server.
- Supporting `SubscribeStream` in the initial implementation; projections and process
  managers use `SubscribeAll`.
- Distributed multi-node or multi-`eventfold-db`-instance deployments.
- Maintaining a client-side stream registry. Stream provenance is carried in event
  metadata, not in local files.

## User Stories

- As an application developer, I want to call
  `AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await` and then
  use `handle.execute(cmd, ctx).await` exactly as before.
- As a `Projection` implementor, I want to receive a `&StoredEvent` with
  `aggregate_type`, `instance_id`, and `recorded_at` already extracted, so I can filter
  and apply events without parsing raw bytes.
- As a developer running multiple processes against the same `eventfold-db` server, I
  want projections in any process to see events from all processes without local
  coordination files.

## Technical Approach

### Stream ID mapping

eventfold-db uses `Uuid` for stream IDs. `eventfold-es` derives them deterministically:

```rust
/// Fixed namespace UUID for stream ID derivation.
const STREAM_NAMESPACE: Uuid = Uuid::from_bytes([
    0x9a, 0x1e, 0x7c, 0x3b, 0x4d, 0x2f, 0x4a, 0x8e,
    0xb5, 0x6c, 0x1f, 0x3d, 0x7e, 0x9a, 0x0b, 0xc4,
]);

/// Derive a deterministic stream UUID from aggregate type and instance ID.
pub fn stream_uuid(aggregate_type: &str, instance_id: &str) -> Uuid {
    let name = format!("{aggregate_type}/{instance_id}");
    Uuid::new_v5(&STREAM_NAMESPACE, name.as_bytes())
}
```

Since UUID v5 is a one-way hash, the reverse mapping is carried in event metadata
(see "Event serialization" below).

### Event serialization

Every event written by `eventfold-es` includes `aggregate_type` and `instance_id` in
the metadata field. This makes each event self-describing -- no external registry needed.

`ProposedEvent` fields (outbound, domain event -> server):
- `event_id`: newly generated UUID v4 string
- `event_type`: the serde tag from the adjacently-tagged `DomainEvent` (e.g., `"Incremented"`)
- `payload`: JSON bytes of the `"data"` field (empty `{}` for unit variants)
- `metadata`: JSON bytes of:
  ```rust
  /// Infrastructure metadata stamped on every event.
  #[derive(Serialize, Deserialize)]
  struct EventMetadata {
      aggregate_type: String,
      instance_id: String,
      #[serde(skip_serializing_if = "Option::is_none")]
      actor: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")]
      correlation_id: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")]
      source_device: Option<String>,
  }
  ```

`StoredEvent` (inbound, server -> projections/PMs):
```rust
/// An event as delivered to projections and process managers.
///
/// All fields are pre-extracted from the gRPC `RecordedEvent` and its
/// JSON metadata. Aggregate type and instance ID are recovered from
/// metadata, not from the stream UUID.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    /// Client-assigned event ID.
    pub event_id: Uuid,
    /// Stream UUID.
    pub stream_id: Uuid,
    /// Aggregate type extracted from metadata (e.g., "counter").
    pub aggregate_type: String,
    /// Instance ID extracted from metadata (e.g., "c-1").
    pub instance_id: String,
    /// Zero-based version within the stream.
    pub stream_version: u64,
    /// Zero-based position in the global log.
    pub global_position: u64,
    /// Event type tag (e.g., "Incremented").
    pub event_type: String,
    /// Decoded JSON payload (the domain event data).
    pub payload: serde_json::Value,
    /// Decoded JSON metadata (actor, correlation_id, etc.).
    pub metadata: serde_json::Value,
    /// Server-assigned timestamp (Unix epoch milliseconds).
    pub recorded_at: u64,
}
```

Events from non-eventfold-es clients (missing `aggregate_type`/`instance_id` in
metadata) are silently skipped by projection and process manager runners.

### Simplified projection and process manager traits

With `aggregate_type` and `instance_id` available on `StoredEvent`, the trait signatures
simplify. Subscription filtering moves into the `apply`/`react` body:

```rust
/// Before
pub trait Projection: Default + Serialize + DeserializeOwned + Send + 'static {
    const NAME: &'static str;
    fn subscriptions(&self) -> &'static [&'static str];
    fn apply(&mut self, aggregate_type: &str, stream_id: &str, event: &eventfold::Event);
}

/// After
pub trait Projection: Default + Serialize + DeserializeOwned + Send + 'static {
    const NAME: &'static str;
    fn apply(&mut self, event: &StoredEvent);
}
```

```rust
/// Before
pub trait ProcessManager: Default + Serialize + DeserializeOwned + Send + 'static {
    const NAME: &'static str;
    fn subscriptions(&self) -> &'static [&'static str];
    fn react(&mut self, aggregate_type: &str, stream_id: &str, event: &eventfold::Event)
        -> Vec<CommandEnvelope>;
}

/// After
pub trait ProcessManager: Default + Serialize + DeserializeOwned + Send + 'static {
    const NAME: &'static str;
    fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>;
}
```

Implementors filter on `event.aggregate_type` or `event.event_type` in the method body.
This is more flexible than the old `subscriptions()` array (which could only match
aggregate types) and reduces trait surface.

### Overview of affected modules

| Module | Change summary |
|--------|----------------|
| `Cargo.toml` | Remove `eventfold = "0.2.0"`; add `tonic`, `prost`, `bytes`, `tokio-stream`, `tonic-build` (build-dep); add `uuid` v5 feature |
| `build.rs` | New file: compile `../eventfold-db/proto/eventfold.proto` via `tonic_build::compile_protos` |
| `src/client.rs` | New module: thin wrapper around the generated `EventStoreClient<Channel>` with typed helpers: `append`, `read_stream`, `subscribe_all_from` |
| `src/event.rs` | New module: `StoredEvent` struct; `EventMetadata` struct; serialization helpers `encode_domain_event` and `decode_stored_event`; `stream_uuid` helper |
| `src/snapshot.rs` | New module: `Snapshot<A>` storing `(state: A, stream_version: u64)` serialized to `<base_dir>/snapshots/<type>/<id>/snapshot.json`; atomic write via temp-rename |
| `src/actor.rs` | Full rewrite: actor holds `(state: A, stream_version: u64, stream_id: Uuid)`; appends via `client.append`; retries up to 3 on `WrongExpectedVersion`; loads snapshot on spawn |
| `src/aggregate.rs` | Remove `to_eventfold_event`, `reducer`, `reduce`; `Aggregate` trait unchanged |
| `src/storage.rs` | Remove or narrow to snapshot directory helpers |
| `src/projection.rs` | Rewrite: `GlobalCursor { position: u64 }` replaces byte-offset cursors; uses `client.subscribe_all_from`; simplified `Projection` trait |
| `src/process_manager.rs` | Rewrite: same cursor changes; simplified `ProcessManager` trait |
| `src/store.rs` | Rewrite: builder-based construction with `endpoint()` + `base_dir()`; holds `EsClient`; `inject_event` maps to gRPC `Append` with `ExpectedVersion::Any`; remove `list_streams` and `read_events` |
| `src/lib.rs` | Remove re-exports of `to_eventfold_event`, `reducer`; add re-exports for `StoredEvent`, `EsClient` |
| `src/error.rs` | Add `ExecuteError::WrongExpectedVersion` and `ExecuteError::Transport(tonic::Status)` |

### Actor lifecycle

1. **Spawn**: Load `Snapshot<A>` from disk (if present), giving `(state, version)`.
   Derive stream UUID via `stream_uuid(A::AGGREGATE_TYPE, instance_id)`.
2. **Catch-up**: Call `client.read_stream(stream_id, snapshot_version + 1, u64::MAX)`
   and fold each event's payload through `A::apply`, updating `version`.
3. **Execute loop** (via tokio mpsc channel):
   - On `Execute { cmd, ctx, reply }`:
     a. Call `state.handle(cmd)` to get `Vec<DomainEvent>`.
     b. Encode to `Vec<ProposedEvent>` (includes `aggregate_type`/`instance_id` in
        metadata).
     c. Call `client.append(stream_id, Exact(version), events)`.
     d. On success: fold events into state, update version, reply Ok.
     e. On `WrongExpectedVersion`: re-read from `version + 1`, re-fold, retry from (a).
        Max 3 retries; then reply `Err(ExecuteError::WrongExpectedVersion)`.
   - On `GetState`: reply with clone of current state.
   - On `Inject { proposed, reply }`: call `client.append(stream_id, Any, [proposed])`.
   - On `Shutdown` or idle timeout: save snapshot; exit.

### Projection catch-up

`ProjectionRunner<P>::catch_up()`:
1. Call `client.subscribe_all_from(last_global_position)`.
2. Read until `CaughtUp` message.
3. For each `RecordedEvent`, decode into `StoredEvent` (parse metadata JSON to extract
   `aggregate_type` and `instance_id`; parse payload JSON).
4. Skip events with missing or unparseable metadata (non-eventfold-es events).
5. Call `projection.apply(&stored_event)`.
6. Save checkpoint with new `global_position`.

### Connection configuration

```rust
let store = AggregateStoreBuilder::new()
    .endpoint("http://127.0.0.1:2113")
    .base_dir("/path/to/local-cache")  // snapshots + checkpoints
    .aggregate_type::<Counter>()
    .projection::<MyProjection>()
    .open()
    .await?;
```

`base_dir` is used only for local snapshots and projection/PM checkpoint files.
Defaults to a system temp directory if not set.

### Files changed (summary)

| File | Action |
|------|--------|
| `Cargo.toml` | Modify |
| `build.rs` | Create |
| `src/client.rs` | Create |
| `src/event.rs` | Create |
| `src/snapshot.rs` | Create |
| `src/actor.rs` | Rewrite |
| `src/aggregate.rs` | Modify |
| `src/storage.rs` | Remove or simplify |
| `src/projection.rs` | Rewrite |
| `src/process_manager.rs` | Rewrite |
| `src/store.rs` | Rewrite |
| `src/error.rs` | Modify |
| `src/lib.rs` | Modify |

## Acceptance Criteria

1. `AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await` returns
   `Ok(store)` when an `eventfold-db` server is listening; returns `Err` when no server
   is reachable.

2. After calling `handle.execute(Increment, ctx).await` three times on the same
   `AggregateHandle<Counter>`, `handle.state().await` returns `Counter { value: 3 }` and
   `client.read_stream(stream_uuid, 0, 100)` returns exactly 3 `RecordedEvent` records
   with `event_type == "Incremented"`.

3. Every `RecordedEvent` written by eventfold-es has metadata containing
   `aggregate_type` and `instance_id` as JSON string fields.

4. Dropping all `AggregateHandle` clones (triggering idle eviction) and then calling
   `store.get::<Counter>("c-1").await` again returns a handle whose state equals the
   state before the drop, confirming the snapshot round-trip.

5. When two concurrent `AggregateStore` instances (same server, separate in-memory state)
   each issue one `Increment` on the same instance ID simultaneously, both `execute`
   calls eventually return `Ok` and `read_stream` returns exactly 2 events.

6. `ProjectionRunner::<EventCounter>::catch_up()` called after two `Increment` events
   from two different aggregate instances returns `state().count == 2`. A second
   `catch_up()` without new writes leaves count unchanged.

7. `ProcessManagerRunner::<EchoSaga>::catch_up()` called after one `Increment` returns
   `Vec<CommandEnvelope>` of length 1. A second call returns empty `Vec`.

8. `store.inject_event::<Counter>("c-1", proposed, InjectOptions::default()).await`
   appends with `ExpectedVersion::Any` and the event is readable from the server.

9. When `client.append` returns `FAILED_PRECONDITION` three consecutive times,
   `handle.execute` returns `Err(ExecuteError::WrongExpectedVersion)`.

10. Every `StoredEvent` delivered to `Projection::apply` has `recorded_at > 0` (Unix
    epoch milliseconds) and non-empty `aggregate_type` and `instance_id` fields.

11. `stream_uuid("counter", "c-1")` produces the same `Uuid` across processes.

12. `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` all pass.

## Open Questions

- **Proto file location**: `build.rs` references `../eventfold-db/proto/eventfold.proto`
  as a relative path. Works in side-by-side checkout; vendoring is a follow-on.

- **Snapshot cache invalidation**: deserialization failure on snapshot load is treated as
  a cache miss (delete and replay from stream). A snapshot version field is deferred.

- **Non-eventfold-es events**: Events written directly to eventfold-db without the
  expected metadata schema are silently skipped by projections and process managers.
  This is intentional -- eventfold-es owns its metadata contract.

## Dependencies

All upstream prerequisites are satisfied. Runtime dependencies:

- `tonic` 0.13 + `prost` 0.13 + `tonic-build` 0.13 (matching eventfold-db).
- `bytes` 1 (now a direct dependency).
- `tokio-stream` 0.1 (for server-streaming RPCs).
- `uuid` 1 with features `["v4", "v5"]`.
- Existing `tokio`, `serde`, `serde_json`, `thiserror`, `tracing` retained.
- `tempfile` 3 in dev-dependencies.
