# Code Review: Ticket 2 -- Add `InjectOptions`, dedup `seen_ids`, and `inject_event` in `src/store.rs`

**Ticket:** 2 -- Add `InjectOptions`, dedup `seen_ids`, and `AggregateStore::inject_event`
**Impl Report:** docs/prd/007-inject-event-reports/ticket-02-impl.md
**Date:** 2026-02-22 22:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `inject_event` returns `Ok(())` and event appears in JSONL | Met | Test `inject_event_appends_to_stream` (line 1495) writes an event, reads back `app.jsonl`, and asserts exactly 1 line. |
| 2 | Projections reflect injected events | Met | Test `inject_event_projections_reflect_event` (line 1517) builds store with `EventCounter` projection, injects 1 event, queries projection and asserts `count == 1`. Catch-up happens in `inject_event` at lines 570-581 via the `ProjectionCatchUpFn` trait. |
| 3 | Duplicate `event.id` is deduplicated | Met | Test `inject_event_dedup_by_id` (line 1537) injects the same event (with `id = Some("ev-1")`) twice, reads JSONL, asserts exactly 1 line. Dedup check at lines 515-523 returns early on duplicate. |
| 4 | Events with `event.id = None` are never deduplicated | Met | Test `inject_event_no_dedup_for_none_id` (line 1568) injects 2 events with `id = None`, asserts 2 lines in JSONL. The `if let Some(ref id)` guard at line 515 correctly skips dedup for `None`. |
| 5 | `InjectOptions::default()` has `run_process_managers: false` | Met | Test `inject_options_default_does_not_run_process_managers` (line 1594) asserts `!opts.run_process_managers`. Doc-test at line 87 also asserts this. `#[derive(Default)]` on the struct (line 98) produces `false` for `bool`. |
| 6 | `InjectOptions { run_process_managers: true }` triggers process managers | Met | Test `inject_event_with_process_managers` (line 1600) injects with `run_process_managers: true`, then verifies `Receiver` aggregate received the dispatched command (`received_count == 1`). |
| 7 | Injecting into a stream with a live actor works without deadlock | Met | Test `inject_event_with_live_actor` (line 1636) spawns an actor via `store.get`, asserts it is alive, injects an event, and verifies `state.value == 1`. The actor path at lines 538-548 routes through `inject_via_actor`. |
| 8 | Injecting into a new stream creates the directory | Met | Test `inject_event_creates_new_stream` (line 1661) injects into `"new-instance"`, asserts the directory exists, then spawns an actor and verifies `state.value == 1`. |
| 9 | `InjectOptions` and `inject_event` re-exported from `lib.rs` with doc comments | Met | `src/lib.rs` line 84: `pub use store::{AggregateStore, AggregateStoreBuilder, InjectOptions};`. `inject_event` is a pub method on `AggregateStore` (already re-exported). `InjectOptions` has doc comments (lines 83-103). `inject_event` has doc comments (lines 473-506). Doc-test at line 87 compiles and passes. |
| 10 | `cargo test` passes | Met | 103 unit tests + 8 doc-tests, all passing. Verified by running `cargo test` directly. |

## Issues Found

### Critical (must fix before merge)
- None

### Major (should fix, risk of downstream problems)
- None

### Minor (nice to fix, not blocking)

1. **Missing `PartialEq` derive on `InjectOptions`** (`src/store.rs` line 98). The `CLAUDE.md` convention says "derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate." Currently only `Debug, Clone, Default` are derived. Adding `PartialEq` is trivial and follows convention.

2. **TOCTOU window in dedup check** (`src/store.rs` lines 515-523, 561-567). The dedup check (lock, check, unlock) and ID insertion (lock, insert, unlock) are separate lock acquisitions with an intervening async write. Two concurrent `inject_event` calls with the same event ID could both pass the dedup check before either inserts the ID. This is a known limitation given the PRD's scope ("in-memory per `AggregateStore` instance") and the design correctly prioritizes data safety (never falsely dedup) over duplicate prevention (duplicates are tolerable). No fix needed for Phase 1.

3. **Projection catch-up runs blocking I/O on async runtime** (`src/store.rs` lines 570-581). The PRD's technical approach suggests wrapping `catch_up()` in `spawn_blocking`, but the implementation calls it directly. This matches the existing pattern in `AggregateStore::projection()` (line 314) which is documented as synchronous. For incremental catch-up of a few events this is fine; for bulk import of many events it could block the reactor. Acceptable for the intended use case.

4. **`#[allow(dead_code)]` on `inject_via_actor` and `ActorMessage::Inject` in `src/actor.rs`** (lines 69, 301). Now that `store.rs` calls `inject_via_actor`, these annotations are no longer needed. The impl report correctly identifies this but leaves them as-is since `actor.rs` is out of scope for this ticket. Should be cleaned up in Ticket 3 or a follow-on.

## Suggestions (non-blocking)

- The `event.clone()` on line 543 could be avoided by restructuring the actor vs. non-actor branches to move `event` into the chosen path rather than cloning for one and moving for the other. For example, an `if/else` that moves `event` into either `inject_via_actor(event)` or the `spawn_blocking` closure. However, the current approach is clear and the cost of cloning a single event is negligible for the intended use case.

- The `ProjectionCatchUpFn` trait and `SharedProjectionCatchUp` wrapper are well-designed and correctly parallel the existing `ProcessManagerCatchUp` pattern. The impl report's concern about adding infrastructure beyond explicit ticket scope is acknowledged but justified -- there's no way to satisfy AC 2 without type-erased catch-up access.

## Scope Check
- Files within scope: YES -- Only `src/store.rs` and `src/lib.rs` were modified, both listed in the ticket scope.
- Scope creep detected: NO -- The new `ProjectionCatchUpFn` trait, `SharedProjectionCatchUp` wrapper, and `ProjectionCatchUpList` type alias are necessary infrastructure to satisfy AC 2 (projection catch-up from `inject_event`). They are internal to the crate and do not modify `projection.rs`.
- Unauthorized dependencies added: NO

## Risk Assessment
- Regression risk: LOW -- All 103 pre-existing unit tests and 8 doc-tests pass. The `ProjectionMap` value type changed from `Box<Mutex<...>>` to `Box<Arc<Mutex<...>>>` but the downcast in `projection()` and `rebuild_projection()` was updated accordingly (lines 330, 367). Existing projection tests verify this still works correctly.
- Security concerns: NONE
- Performance concerns: NONE for typical use. The synchronous projection catch-up (Minor #3) is the only theoretical concern for bulk import scenarios, but matches the established codebase pattern.

## Verification Commands Run
- `cargo test` -- PASS (103 unit tests + 8 doc-tests)
- `cargo clippy -- -D warnings` -- PASS (zero warnings)
- `cargo fmt --check` -- PASS (no formatting issues)
