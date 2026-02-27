# Implementation Report: Ticket 6 -- src/storage.rs: narrow to snapshot directory helpers only

**Ticket:** 6 - src/storage.rs: narrow to snapshot directory helpers only
**Date:** 2026-02-27 18:30
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/storage.rs` - Complete rewrite: stripped down from 450 lines to 114 lines. Removed all JSONL-era methods (stream_dir, views_dir, ensure_stream, list_streams, list_aggregate_types, meta_dir, registry logic). Added new `snapshots_dir()` method. Changed visibility from `pub` to `pub(crate)`. Removed all unused imports (fs, OpenOptions, BufRead, BufReader, Write, SystemTime). Added `#![allow(dead_code)]` module-level attribute (matching snapshot.rs pattern) since no consumer exists yet.
- `src/lib.rs` - Uncommented `mod storage;`. Did NOT add `pub use` for StreamLayout (it is `pub(crate)`). Removed the old commented-out `pub use storage::StreamLayout` line.

## Implementation Notes
- Followed the same `#![allow(dead_code)]` pattern as `src/snapshot.rs` for suppressing dead_code warnings on `pub(crate)` items not yet consumed by other modules.
- The old storage.rs had 7 methods and 8 tests heavily dependent on JSONL infrastructure (filesystem I/O, serde_json for registry parsing, BufReader, SystemTime). The new module has 5 methods and 4 tests with zero external dependencies beyond `std::path`.
- All imports from `std::fs`, `std::io`, and `std::time` were removed since the remaining methods are pure path-computation helpers with no I/O.

## Acceptance Criteria
- [x] AC 1: StreamLayout retains only: new(), base_dir(), projections_dir(), process_managers_dir(), and snapshots_dir() - All five methods present, no others.
- [x] AC 2: All JSONL-era methods removed - stream_dir, views_dir, ensure_stream, list_streams, list_aggregate_types, meta_dir, and registry file logic are all gone.
- [x] AC 3: StreamLayout visibility changed to pub(crate) - Struct and all methods are `pub(crate)`.
- [x] AC 4: All tests updated to only test remaining methods - 4 tests covering the 4 path-returning methods; all 8 old JSONL-related tests removed.
- [x] AC 5: Test: layout.snapshots_dir() returns `<base_dir>/snapshots` - `snapshots_dir_returns_base_dir_slash_snapshots` test.
- [x] AC 6: Test: layout.projections_dir() returns `<base_dir>/projections` - `projections_dir_returns_base_dir_slash_projections` test.
- [x] AC 7: Test: layout.process_managers_dir() returns `<base_dir>/process_managers` - `process_managers_dir_returns_base_dir_slash_process_managers` test.
- [x] AC 8: Quality gates pass - All four gates pass (see below).

## Test Results
- Fmt: PASS (`cargo fmt --check` -- no output)
- Build: PASS (`cargo build` -- zero warnings)
- Clippy: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (66 unit tests + 3 doc-tests, all passing)
- New tests added:
  - `src/storage.rs::tests::base_dir_returns_root`
  - `src/storage.rs::tests::snapshots_dir_returns_base_dir_slash_snapshots`
  - `src/storage.rs::tests::projections_dir_returns_base_dir_slash_projections`
  - `src/storage.rs::tests::process_managers_dir_returns_base_dir_slash_process_managers`

## Concerns / Blockers
- None
