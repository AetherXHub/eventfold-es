# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/aetherxhub/eventfold-es/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/aetherxhub/eventfold-es/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aetherxhub/eventfold-es/releases/tag/v0.1.0
