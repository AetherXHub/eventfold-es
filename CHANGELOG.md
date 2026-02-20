# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/aetherxhub/eventfold-es/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/aetherxhub/eventfold-es/releases/tag/v0.1.0
