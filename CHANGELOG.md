# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-02-27

### Changed

- **Breaking:** Replaced `eventfold` JSONL file backend with `eventfold-db` gRPC
  client. All event storage now goes through a remote eventfold-db server.
- **Breaking:** `AggregateStore::open(base_dir)` replaced by
  `AggregateStoreBuilder::new().endpoint(url).base_dir(path).open()`.
- **Breaking:** Removed `CommandBus` and typed command routing. Use
  `store.get::<A>(id).execute(cmd, ctx)` directly.
- **Breaking:** `Projection::apply()` and `ProcessManager::react()` now receive
  `&StoredEvent` instead of `&eventfold::Event`.
- **Breaking:** `AggregateHandle::reader()` removed (no local event log).
- Actors are now tokio tasks (not blocking threads) with optimistic concurrency
  control (3 automatic retries on `WrongExpectedVersion`).
- Projections and process managers consume the global event log via `SubscribeAll`
  with `global_position` cursors (replacing per-stream byte-offset cursors).
- `StreamLayout` narrowed to `pub(crate)` with only snapshot/checkpoint paths.

### Added

- `EsClient` -- typed gRPC client wrapping tonic-generated `EventStoreClient`.
- `ExpectedVersionArg` enum (Any, NoStream, Exact) for optimistic concurrency.
- `StoredEvent` struct with aggregate type, instance ID, global position, and
  server-assigned `recorded_at` timestamp.
- `EventMetadata` struct for structured event metadata (aggregate type, instance
  ID, actor, correlation ID, source device).
- `stream_uuid()` -- deterministic UUID v5 mapping from (aggregate_type,
  instance_id) to stream ID.
- `encode_domain_event()` / `decode_stored_event()` for domain event
  serialization to/from gRPC payloads.
- `Snapshot<A>` with atomic file-based save/load for fast actor recovery.
- `EventStoreOps` trait for actor testability without a live gRPC connection.
- Vendored `proto/eventfold.proto` for crates.io publishing.

### Removed

- `eventfold` crate dependency (JSONL file-based storage).
- `CommandBus`, `CommandRoute`, `TypedCommandRoute`.
- `Aggregate::reducer()`, `Aggregate::reduce()`, `to_eventfold_event()`.
- Per-stream directory hierarchy (`streams/<type>/<id>/app.jsonl`, `views/`).
- Stream registry file (`meta/streams.jsonl`).

### Dependencies

- Added `tonic` 0.13, `prost` 0.13, `bytes` 1, `tokio-stream` 0.1.
- Added `uuid` `v5` feature (previously only `v4`).
- Added `tonic-build` 0.13 as build dependency.
- Removed `eventfold` 0.2.0.

## [0.2.0] - 2026-02-22

### Added

- `AggregateStore::rebuild_projection<P>()` to rebuild a single projection from
  scratch without restarting the application
- Auto-generated UUID v4 event IDs in `to_eventfold_event()` for deduplication
  during multi-device sync
- `CommandContext::source_device` field and `with_source_device()` builder for
  stamping originating device identity into `event.meta`
- `AggregateStore::inject_event()` to append pre-validated events directly to
  streams, bypassing command validation (for sync/replication scenarios)
- `InjectOptions` struct to control process manager triggering during injection
- In-memory event ID deduplication via `seen_ids` on `AggregateStore`
- `AggregateStore::list_streams()` to enumerate all aggregate instances,
  optionally filtered by type
- `AggregateStore::read_events()` to read raw events from a stream
- `ActorMessage::Inject` variant for actor-safe event injection without lock
  contention

### Dependencies

- Added `uuid` 1.x with `v4` feature

## [0.1.0] - 2026-02-19

### Added

- Core aggregate loop with `Aggregate` trait, command handling, and event application
- `EventStore` backed by `eventfold` JSONL event log
- `Projection` trait for deriving read models from event streams
- `ProcessManager` trait for cross-aggregate workflow orchestration
- `CommandHandler` for dispatching commands through aggregates
- `ActorContext` for tracking command actors
- Custom error types via `thiserror`
- Async support via `tokio`
- README with API overview, architecture, and usage examples

[Unreleased]: https://github.com/aetherxhub/eventfold-es/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/aetherxhub/eventfold-es/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/aetherxhub/eventfold-es/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aetherxhub/eventfold-es/releases/tag/v0.1.0
