# Code Review: Ticket 3 -- src/error.rs: add Transport and WrongExpectedVersion variants

**Ticket:** 3 -- src/error.rs: add Transport and WrongExpectedVersion variants
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-03-impl.md
**Date:** 2026-02-27 16:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `ExecuteError::WrongExpectedVersion` variant exists (replaces `Conflict`) | Met | Line 25 of `src/error.rs`: `Conflict` renamed to `WrongExpectedVersion` with updated error message `"wrong expected version: retries exhausted"`. Ticket AC says old message is acceptable or update allowed -- updated message is appropriate for the new variant name. |
| 2 | `ExecuteError::Transport(tonic::Status)` exists with `#[error("gRPC transport error: {0}")]` | Met | Lines 38-39 of `src/error.rs`: variant added with exact error format string. Doc comment explains the purpose. |
| 3 | `StateError::Transport(tonic::Status)` exists with `#[error("gRPC transport error: {0}")]` | Met | Lines 63-64 of `src/error.rs`: identical pattern to `ExecuteError::Transport`. Doc comment present. |
| 4 | Existing tests updated for new variant names; no tests removed for non-conflicting variants | Met | `execute_error_conflict_display` renamed to `execute_error_wrong_expected_version_display_message` (line 123). `Domain`, `Io`, `ActorGone` tests all preserved unchanged at lines 117, 129, 136, 142, 149. |
| 5 | Test: `ExecuteError::WrongExpectedVersion` `.to_string()` returns non-empty string | Met | `execute_error_wrong_expected_version_display` test at line 155 asserts `!msg.is_empty()`. (Technically subsumed by AC 4's test which checks exact equality, but both are present per AC requirements.) |
| 6 | Test: `ExecuteError::Transport(tonic::Status::internal("boom"))` `.to_string()` contains "boom" or "gRPC" | Met | `execute_error_transport_display` test at line 162 constructs with `tonic::Status::internal("boom")` and asserts `msg.contains("boom") || msg.contains("gRPC")`. |
| 7 | `Send + Sync` compile-time assertions pass for `ExecuteError<TestDomainError>` and `StateError` | Met | Const assertion block at lines 185-194 compiles. `tonic::Status` is `Send + Sync`, so adding `Transport(tonic::Status)` does not break the bounds. Confirmed by `cargo test` passing. |
| 8 | Quality gates pass (build, lint, fmt, tests) | Met | Verified: `cargo test` (52 passed + 2 doc-tests), `cargo clippy -- -D warnings` (zero warnings), `cargo fmt --check` (no diffs), `cargo build` (clean). |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

- **Redundant test** (`src/error.rs`, line 155): `execute_error_wrong_expected_version_display` only asserts `!msg.is_empty()`, which is strictly weaker than `execute_error_wrong_expected_version_display_message` at line 123 which asserts exact string equality. Both exist because the ACs specify them separately. Not harmful, just redundant coverage.

## Suggestions (non-blocking)

- The `StateError::Transport` test at line 173 uses `tonic::Status::unavailable("server down")` while the `ExecuteError::Transport` test uses `tonic::Status::internal("boom")`. Good practice to vary the status codes -- nicely done.
- No `#[from]` on `Transport` variants is a deliberate design choice documented in the impl report. This is correct -- `tonic::Status` should be explicitly wrapped, not auto-converted, since a `Status` could represent many different error conditions and blind conversion would lose context for the caller.

## Scope Check

- Files within scope: YES
  - `src/error.rs`: in ticket scope (Modify)
  - `src/lib.rs`: not listed in ticket scope, but the change is limited to uncommenting `mod error;` and `pub use error::{...};` which were commented out by prior tickets (1 and 2) during the migration. This is a necessary enablement change, not scope creep. The error module was active in the committed v0.2.0 `lib.rs`; Tickets 1-2 commented it out as part of the bulk migration, and Ticket 3 re-enables it. The vast majority of the `lib.rs` diff belongs to Tickets 1 and 2.
- Scope creep detected: NO
- Unauthorized dependencies added: NO (`tonic` was already added by Ticket 1)

## Risk Assessment

- Regression risk: LOW -- This is an additive change (new variants) with one rename (`Conflict` -> `WrongExpectedVersion`). No callers of the old `Conflict` variant exist in the codebase since the actor module is commented out during migration. The rename is safe.
- Security concerns: NONE
- Performance concerns: NONE
