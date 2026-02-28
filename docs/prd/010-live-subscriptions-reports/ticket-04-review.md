# Code Review: Ticket 4 -- AggregateStore start_live() and live_projection()

**Ticket:** 4 -- AggregateStore start_live() and live_projection()
**Impl Report:** docs/prd/010-live-subscriptions-reports/ticket-04-impl.md
**Date:** 2026-02-27 15:45
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `store.start_live().await` returns `Ok(handle)` | Met | Test `start_live_returns_ok_handle` (store.rs:1069) creates a mock store, calls `start_live()`, asserts `result.is_ok()`, and cleans up via `shutdown()`. Implementation at store.rs:227-253 spawns the tokio task and returns a clone of the handle. |
| 2 | Second `store.start_live().await` returns `Err` with `ErrorKind::AlreadyExists` | Met | Test `start_live_twice_returns_already_exists_error` (store.rs:791) calls `start_live()` twice and asserts the second returns `ErrorKind::AlreadyExists`. Implementation at store.rs:229-233 checks `guard.is_some()` under the mutex. |
| 4/5 | `store.live_projection::<P>()` returns state without catch-up | Met | Test `live_projection_returns_state_when_live_active` (store.rs:811) manually applies 3 events via `apply_event()`, places a `LiveHandle` in the mutex (simulating active live mode), then calls `live_projection()` and asserts `count == 3`. No gRPC call is made -- the test uses a lazy non-connecting client, confirming no catch-up occurs. |
| 12 | `live_projection()` falls back to pull-based catch-up when live mode inactive | Met | Test `live_projection_falls_back_when_not_live` (store.rs:909) calls `live_projection()` without starting live mode on an unregistered projection and asserts `ErrorKind::NotFound` -- the same error `projection()` would return, proving the fallback path is taken. |
| 12 | `projection()` works whether or not live mode is active | Met | Tests `projection_works_when_live_mode_active` (store.rs:941) and `projection_works_when_live_mode_not_active` (store.rs:984) both exercise `projection()` and verify it returns `NotFound` for unregistered projections regardless of live handle state. The `projection()` method at store.rs:314-329 has no dependency on `live_handle`. |
| 14 | `start_live` and `live_projection` have doc comments | Met | `start_live` has comprehensive doc comments at store.rs:211-226 covering purpose, returns, and errors. `live_projection` has doc comments at store.rs:256-274 covering live vs fallback behavior, returns, and errors. Both follow the project's established `///` + `# Returns` + `# Errors` pattern. |
| -- | `cargo test` passes | Met | Verified: 122 unit tests + 6 doc-tests all pass. |
| -- | `cargo clippy -- -D warnings` passes | Met | Verified: clean output, zero warnings. |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **`#[allow(dead_code)]` on `ProjectionCatchUp` trait methods** (projection.rs:296, 300, 308): The `apply_event`, `position`, and `save` methods have `#[allow(dead_code)]` annotations with comments "Will be consumed by the live subscription loop." As of this ticket, these methods ARE consumed by `live.rs` (`process_stream_with_dispatch`, `save_all_checkpoints`, `min_global_position`). The annotations and comments are now stale and should be removed. Similarly, `ProjectionRunner::state()` at line 175 has `#[allow(dead_code)]` noting it was "Superseded by ProjectionCatchUp::state_any" -- this should be removed if it is truly dead code, or left if it has value as a debug/test accessor. These are in `projection.rs` which is outside Ticket 4's scope, so deferring is acceptable -- but they should be cleaned up in Ticket 5 (verification).

2. **`result.unwrap()` in test code** (store.rs:1074): The test `start_live_returns_ok_handle` calls `result.unwrap()` after already asserting `result.is_ok()`. This is functionally fine in test code but could use `result.expect("...")` for consistency with the project's convention (other tests use `expect()` directly). Very minor.

## Suggestions (non-blocking)

1. **PRD divergence on `projection()` behavior during live mode**: The PRD (line 278) suggests `store.projection::<P>()` should skip catch-up when live mode is active ("equivalent to `store.live_projection::<P>()`"). The ticket AC says it should "still work" -- and the implementation always does a full catch-up regardless of live mode. This is a defensible choice (simpler, more predictable) but worth documenting the deliberate deviation. The PRD's suggestion would be a performance optimization for future consideration.

2. **`live_projection` lock ordering**: The method acquires `live_handle` mutex first, then drops it, then acquires `projections` RwLock. This is correct and avoids deadlocks. However, between dropping `live_handle` and acquiring `projections`, another task could call `shutdown()` and drain the live loop. In practice this is fine because `live_projection` would still read valid (if stale) state, but a code comment noting this benign race would help future readers.

## Scope Check

- Files within scope: YES
  - `src/store.rs` -- primary target, correctly modified
  - `src/live.rs` -- field visibility changes (`pub(crate)`) and test helper updates for `live_handle` field were mechanically necessary for `start_live()` to construct `LiveHandle`. The impl report correctly flags this as a deviation and justifies it.
- Scope creep detected: NO -- All changes directly serve the ticket's ACs. The `live.rs` changes are minimal and necessary.
- Unauthorized dependencies added: NO

Note: The working tree contains co-mingled changes from Tickets 1-4 (all uncommitted). Changes in `projection.rs`, `process_manager.rs`, and `lib.rs` belong to prior tickets. The Ticket 4-specific changes are confined to `store.rs` (new methods, field, tests) and `live.rs` (field visibility + test helpers).

## Risk Assessment

- Regression risk: LOW -- `projection()` method was refactored in Ticket 2 (prior ticket) to use `state_any()` instead of direct downcasting. All existing tests pass. The new `live_handle` field is initialized to `None` and is inert until `start_live()` is called. No existing code paths are affected.
- Security concerns: NONE
- Performance concerns: NONE -- `live_projection` acquires one Mutex (brief check) and one RwLock (read path only), both released quickly. The `state_any()` call clones the projection state, which is the expected cost.
