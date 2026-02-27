# Code Review: Ticket 10 -- src/process_manager.rs: rewrite to gRPC SubscribeAll cursor

**Ticket:** 10 -- src/process_manager.rs: rewrite to gRPC SubscribeAll cursor
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-10-impl.md
**Date:** 2026-02-27 21:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | ProcessManager trait: no `subscriptions()`, react takes `&StoredEvent` returns `Vec<CommandEnvelope>` | Met | Trait at line 47 has `const NAME` and `fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>`. No `subscriptions()` method exists. |
| 2 | ProcessManagerCheckpoint uses `last_global_position: u64` | Met | `ProcessManagerCheckpoint<PM>` at line 78 has fields `state: PM` and `last_global_position: u64`. Serializes to `{ "state": ..., "last_global_position": N }`. |
| 3 | ProcessManagerRunner holds `client: EsClient` and `checkpoint_dir: PathBuf` | Met | Struct at line 170 has fields `checkpoint`, `client: crate::client::EsClient`, and `checkpoint_dir: PathBuf`. |
| 4 | `catch_up` consumes SubscribeAll until CaughtUp, decodes, reacts, collects envelopes; does NOT auto-save | Met | `catch_up()` (line 223) calls `subscribe_all_from`, delegates to `process_stream()` (line 247) which loops until `CaughtUp`, calls `decode_stored_event`, calls `checkpoint.state.react(&stored)`, collects envelopes. No `save` call in `catch_up`. |
| 5 | `save` persists `last_global_position` and PM state | Met | `save()` at line 304 delegates to `save_pm_checkpoint()` which serializes `ProcessManagerCheckpoint` (containing both `state` and `last_global_position`) to disk atomically via temp-rename. |
| 6 | ProcessManagerCatchUp trait object interface updated | Met | `ProcessManagerCatchUp` trait at line 327 returns `Pin<Box<dyn Future<Output = io::Result<Vec<CommandEnvelope>>> + Send + '_>>` for `catch_up`. `save`, `name`, `dead_letter_path` methods match. Blanket impl for `ProcessManagerRunner<PM>` at line 353. |
| 7 | Test: one valid event -> 1 envelope, updates position | Met | `catch_up_with_one_valid_event_returns_one_envelope` (line 444): asserts `envelopes.len() == 1`, `aggregate_type == "target"`, `instance_id == "c-1"`, `events_seen == 1`, `last_global_position == 1`. |
| 8 | Test: second catch_up -> empty Vec | Met | `second_catch_up_with_saved_checkpoint_returns_empty` (line 459): first pass processes one event, second pass gets only CaughtUp, asserts `envelopes.is_empty()`, `events_seen == 1`, `last_global_position == 1`. |
| 9 | Test: non-ES events skipped | Met | `non_es_events_skipped_returns_empty` (line 477): RecordedEvent with `metadata = b"{}"` (no aggregate_type/instance_id), asserts `envelopes.is_empty()`, `events_seen == 0`, `last_global_position == 6` (position still advances). |
| 10 | Test: checkpoint serialization roundtrip | Met | `checkpoint_serialization_roundtrip` (line 416): creates checkpoint with `events_seen: 5, last_global_position: 42`, serializes to JSON, asserts fields in parsed JSON, then full roundtrip deserialization back to struct. |
| 11 | Quality gates pass | Met | `cargo test`: 91 tests pass + 3 doc-tests. `cargo clippy -- -D warnings`: clean. `cargo fmt --check`: clean. `cargo build`: clean. Verified by reviewer. |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **Projection uses `tracing::debug_span!().entered()` across await; PM does not.** The projection's `process_stream` (line 224 of `projection.rs`) holds an `EnteredSpan` across the `while let` loop with `.await` calls. This works because `ProjectionRunner::process_stream` is not boxed as `Send`. The PM implementer correctly avoided this pattern because `ProcessManagerCatchUp::catch_up` returns a `Send` boxed future, which would fail to compile with a held `EnteredSpan`. The substitution of `tracing::debug!()` at line 249 is functionally correct, but it loses the structured span context that the projection enjoys. If a span is desired in the future, `tracing::Instrument::instrument()` is the idiomatic approach for async Send contexts.

2. **Dead-letter `SystemTime::UNIX_EPOCH.elapsed()` direction.** At line 386, `append_dead_letter` uses `SystemTime::UNIX_EPOCH.elapsed()` which computes `UNIX_EPOCH.elapsed()` -- this is `SystemTime::now() - UNIX_EPOCH` expressed differently. It works, but is less immediately readable than the conventional `SystemTime::now().duration_since(UNIX_EPOCH)` pattern used elsewhere in the codebase (per CLAUDE.md: `SystemTime::now().duration_since(UNIX_EPOCH).expect(...)` is the correct pattern for invariant violations). The `.expect()` message is good.

## Suggestions (non-blocking)

- The `test_fixtures` module is `pub(crate)` which is correct for cross-module test reuse. The `EchoSaga` fixture is well-designed: it filters by `aggregate_type == "counter"`, tracks `events_seen`, and emits a targeting envelope with correlation ID. Good test design.

- The `process_stream` factoring (extracting stream processing from `catch_up` so tests can inject mock streams) correctly mirrors the projection pattern and is clean.

- 10 new tests cover: trait shape, checkpoint serialization (roundtrip + default), checkpoint persistence (save/load roundtrip, load empty, load corrupt), stream processing (valid event, idempotent second pass, non-ES skip), and dead-letter append. This is thorough coverage.

## Scope Check

- Files within scope: YES -- `src/process_manager.rs` (full rewrite) and `src/lib.rs` (re-exports maintained).
- Scope creep detected: NO -- The `src/lib.rs` changes visible in git diff are from the cumulative PRD-009 migration (Tickets 1-9), not from Ticket 10. The process-manager-specific lines (`mod process_manager;` and `pub use process_manager::{ProcessManager, ProcessManagerReport};`) are unchanged from the pre-existing state.
- Unauthorized dependencies added: NO -- No new Cargo.toml changes for this ticket.

## Risk Assessment

- Regression risk: LOW -- The process manager module was already a complete rewrite (old `eventfold`-based code replaced). The `store` module that consumes it is currently commented out (`// mod store;` in lib.rs). No other modules depend on `ProcessManagerRunner` or `ProcessManagerCatchUp`. The public API exports (`ProcessManager` trait, `ProcessManagerReport`) maintain the same contract.
- Security concerns: NONE
- Performance concerns: NONE -- Stream processing is O(n) over events. Dead-letter append is O(1) per entry. Checkpoint save/load is atomic file I/O. All appropriate for the use case.
