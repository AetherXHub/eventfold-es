# Code Review: Ticket 1 -- Add position_tx field, subscribe() method, start_live() wiring

**Ticket:** 1 -- Add position_tx field to LiveHandle, subscribe() method, and start_live() wiring
**Impl Report:** docs/prd/012-live-handle-position-channel-reports/ticket-01-impl.md
**Date:** 2026-02-28 15:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `LiveHandle` has `pub(crate) position_tx: Arc<tokio::sync::watch::Sender<u64>>` immediately after `caught_up`, with doc comment | Met | Field at `src/live.rs:93-95`, placed immediately after `caught_up` (line 92). Doc comment matches PRD spec: "Carries the latest global position after each event is processed. Initialized to 0. Callers obtain a receiver via `subscribe`." |
| 2 | `pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64>` delegates to `self.position_tx.subscribe()`, with full doc comment including `# Usage` example | Met | Method at `src/live.rs:112-139`. Doc comment includes `# Usage` section with `no_run` example showing `changed().await` + `borrow_and_update()` pattern. Doc-test compiles (verified in cargo test output). |
| 3 | `LiveHandle` still derives `Clone` without manual impl | Met | `#[derive(Clone)]` remains at `src/live.rs:87`. `Arc<watch::Sender<u64>>` is `Clone`. Verified by `cargo build` success and the `live_handle_is_clone` test passing. |
| 4 | `start_live()` creates `watch::channel(0u64)`, wraps in Arc, clones into task, includes in LiveHandle literal | Met | `src/store.rs:237-256`. Channel created at line 237, wrapped in Arc at line 238, cloned into spawned task at line 243-247 (via `let _position_tx = position_tx_clone;` to keep sender alive), included in LiveHandle literal at line 254. |
| 5 | All existing `LiveHandle` struct literals in test code compile with new field | Met | All 6 test literals in `src/live.rs` (lines 494, 506, 519, 532, 611, 976) and 2 test literals in `src/store.rs` (lines 901, 976) updated with `position_tx: Arc::new(tokio::sync::watch::channel(0u64).0)`. All 125 tests pass. |
| 6 | Test: `subscribe_returns_receiver_with_initial_value_zero` | Met | `src/live.rs:540-551`. Constructs bare LiveHandle, calls `subscribe()`, asserts `*rx.borrow() == 0`. Test passes. |
| 7 | Test: `subscribe_multiple_times_returns_independent_receivers` | Met | `src/live.rs:553-578`. Creates two receivers, verifies both start at 0, sends 42 via `position_tx`, verifies both see 42 via `borrow_and_update()`. Test passes. |
| 8 | Test: `subscribe_on_cloned_handle_shares_same_channel` | Met | `src/live.rs:580-602`. Clones handle, asserts `Arc::ptr_eq` on `position_tx`, subscribes on clone, sends 99 from original, verifies clone's receiver sees 99. Test passes. |
| 9 | `cargo build --locked` exits 0 with zero warnings | Met | Verified: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.03s` |
| 10 | All pre-existing tests pass | Met | 125 unit tests + 7 doc-tests all green. No assertion changes to existing tests -- only the `position_tx` field was added to struct literals. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)
- None

## Suggestions (non-blocking)
- The 3 new test functions in `src/live.rs` (lines 540-602) are placed between `shutdown_with_no_task_returns_ok` and `shutdown_twice_returns_ok`, splitting the shutdown tests apart. Grouping all `subscribe_*` tests together after the shutdown tests would improve readability, but this is purely organizational preference.

## Scope Check
- Files within scope: YES -- only `src/live.rs` and `src/store.rs` modified, exactly matching the ticket's listed file scope.
- Scope creep detected: NO
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- Only additive changes (new field, new method, new tests). All 125 pre-existing unit tests and 7 doc-tests pass unchanged. The `position_tx_clone` moved into the spawned task is correctly kept alive via `let _position_tx` binding, ensuring the watch channel remains open for future wiring in Ticket 2.
- Security concerns: NONE
- Performance concerns: NONE -- `watch::channel` is lightweight; the `Arc` wrapper adds negligible overhead. The unused `_position_rx` from channel creation is immediately dropped (correct -- receivers subscribe via `subscribe()` method, not by cloning the initial receiver).

## Verification
- `cargo build --locked`: PASS (zero warnings)
- `cargo test --locked`: PASS (125 unit + 7 doc-tests, all green)
- `cargo fmt --check`: PASS (no diffs)
- `cargo clippy --all-targets --all-features --locked -- -D warnings`: 4 pre-existing `result_large_err` errors in `src/projection.rs` and `src/process_manager.rs` (not in modified files; confirmed identical on main branch). No new warnings introduced by this ticket.
