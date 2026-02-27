# Implementation Report: Ticket 3 -- src/error.rs: add Transport and WrongExpectedVersion variants

**Ticket:** 3 - src/error.rs: add Transport and WrongExpectedVersion variants
**Date:** 2026-02-27 15:30
**Status:** COMPLETE

---

## Files Changed

### Modified
- `src/error.rs` - Renamed `ExecuteError::Conflict` to `ExecuteError::WrongExpectedVersion`; added `ExecuteError::Transport(tonic::Status)` and `StateError::Transport(tonic::Status)` variants; updated existing test for renamed variant; added 3 new tests for new variants.
- `src/lib.rs` - Uncommented `mod error;` and `pub use error::{DispatchError, ExecuteError, StateError};` to make the error module active.

## Implementation Notes
- The `Conflict` variant was renamed to `WrongExpectedVersion` with an updated error message: `"wrong expected version: retries exhausted"` (changed from `"optimistic concurrency conflict: retries exhausted"`).
- `tonic::Status` is `Send + Sync`, so the compile-time assertions continue to pass without changes.
- No `#[from]` attribute was added to the `Transport` variants since `tonic::Status` is not a standard error conversion target -- callers will construct `Transport(status)` explicitly.
- The `DispatchError` enum was left untouched as the ticket scope only covers `ExecuteError` and `StateError`.
- The error module had no `eventfold` imports to begin with, so no removal was needed.
- Only `mod error` and its `pub use` line were uncommented in `lib.rs`. All other commented-out modules remain disabled as they still depend on types not yet migrated.

## Acceptance Criteria
- [x] AC 1: `ExecuteError::WrongExpectedVersion` variant exists (replaces `Conflict`; updated error message) - Renamed on line 25 of error.rs
- [x] AC 2: `ExecuteError::Transport(tonic::Status)` variant exists with `#[error("gRPC transport error: {0}")]` - Added on lines 38-39 of error.rs
- [x] AC 3: `StateError::Transport(tonic::Status)` variant exists with `#[error("gRPC transport error: {0}")]` - Added on lines 63-64 of error.rs
- [x] AC 4: All existing tests updated to reference new variant names; no tests removed for non-conflicting variants - `execute_error_conflict_display` renamed to `execute_error_wrong_expected_version_display_message` and updated; Domain, Io, ActorGone tests unchanged
- [x] AC 5: Test: constructing `ExecuteError::WrongExpectedVersion` and calling `.to_string()` returns a non-empty string - `execute_error_wrong_expected_version_display` test on line 154
- [x] AC 6: Test: constructing `ExecuteError::Transport(tonic::Status::internal("boom"))` and calling `.to_string()` contains "boom" or "gRPC" - `execute_error_transport_display` test on line 161
- [x] AC 7: Send + Sync compile-time assertions still pass for `ExecuteError<TestDomainError>` and `StateError` - Existing const assertion block on lines 185-194 compiles and passes
- [x] AC 8: Quality gates pass (build, lint, fmt, tests) - All pass, see below

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (`cargo test` -- 16 passed, 0 failed)
- Build: PASS (`cargo build` -- zero warnings)
- Fmt: PASS (`cargo fmt --check` -- no diffs)
- New tests added:
  - `error::tests::execute_error_wrong_expected_version_display` (line 154)
  - `error::tests::execute_error_transport_display` (line 161)
  - `error::tests::state_error_transport_display` (line 172)
- Updated tests:
  - `error::tests::execute_error_wrong_expected_version_display_message` (renamed from `execute_error_conflict_display`, line 122)

## Concerns / Blockers
- None
