# Code Review: Ticket 9 -- src/projection.rs: rewrite to gRPC SubscribeAll cursor

**Ticket:** 9 -- src/projection.rs: rewrite to gRPC SubscribeAll cursor
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-09-impl.md
**Date:** 2026-02-27 18:45
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | Projection trait loses subscriptions(); apply takes &StoredEvent | Met | Line 35-47: trait has only `NAME` and `fn apply(&mut self, event: &StoredEvent)`. Old `subscriptions()` and 3-param `apply` removed. |
| 2 | ProjectionCheckpoint uses last_global_position: u64 | Met | Lines 54-63: `state: P` + `last_global_position: u64`. Test `checkpoint_serializes_with_last_global_position` confirms JSON shape `{"state":...,"last_global_position":N}`. |
| 3 | ProjectionRunner holds client: EsClient and checkpoint_dir: PathBuf | Met | Lines 138-145: `client: crate::client::EsClient`, `checkpoint_dir: PathBuf`. No `StreamLayout`. |
| 4 | new() loads checkpoint or defaults to position 0 | Met | Lines 163-173: `load_checkpoint::<P>(&checkpoint_dir)?.unwrap_or_default()`. Default impl at lines 65-72 sets position to 0. |
| 5 | catch_up() calls subscribe_all_from, reads until CaughtUp, decodes, applies, saves | Met | Lines 198-208: opens subscription, delegates to `process_stream`, saves checkpoint. Lines 219-258: iterates stream, matches Event/CaughtUp/None, decodes via `decode_stored_event`, applies, advances cursor. |
| 6 | Non-ES events silently skipped | Met | Line 236: `if let Some(stored) = decode_stored_event(&recorded)` -- None results skip apply. Line 245: cursor still advances past skipped events. No panic, no error. |
| 7 | Test: 2 events + CaughtUp -> count == 2, correct position | Met | `catch_up_fresh_checkpoint_with_two_events` (lines 431-446): asserts `count == 2` and `last_global_position == 2`. |
| 8 | Test: second catch_up with only CaughtUp leaves count unchanged | Met | `second_catch_up_with_only_caught_up_leaves_count_unchanged` (lines 449-463): starts at count=2/pos=2, receives only CaughtUp, asserts both unchanged. |
| 9 | Test: metadata = b"{}" is skipped | Met | `recorded_event_with_empty_metadata_is_skipped` (lines 466-495): constructs RecordedEvent with empty JSON metadata, asserts count stays 0 but position advances to 6. |
| 10 | Test: checkpoint save/load roundtrip | Met | `checkpoint_save_load_roundtrip_via_process_stream` (lines 498-522) plus `save_then_load_roundtrips` (lines 374-388) and `checkpoint_serde_roundtrip` (lines 361-371). All verify state and position survive serialization. |
| 11 | Quality gates pass | Met | cargo build: 0 warnings. cargo test: 81 passed + 3 doctests. cargo clippy -- -D warnings: clean. cargo fmt --check: clean. |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **`Projection` trait bounds include `Clone + Sync` not shown in PRD**: The PRD "After" signature shows `Default + Serialize + DeserializeOwned + Send + 'static`, but the implementation adds `Clone + Sync`. This is inherited from the old trait definition and is defensible (Clone needed for checkpoint state, Sync for cross-task sharing), but is worth noting for documentation accuracy. Not blocking since the old trait already had these bounds.

## Suggestions (non-blocking)

1. **`save_checkpoint` is sync I/O inside `catch_up` (async fn)**: `save_checkpoint` at line 206 calls `std::fs::create_dir_all`, `std::fs::write`, and `std::fs::rename` from within an async context. For small checkpoint files this is fine, but if the store module later calls `catch_up` from a Tokio task, consider wrapping in `spawn_blocking`. Low priority since checkpoint files are tiny JSON.

2. **`process_stream` span lifetime**: The `_span` at line 224 uses `.entered()` which ties the span to the current thread. In async code, `tracing::Instrument` (`.instrument(span)`) is generally preferred so the span follows the task across `await` points. Currently acceptable since `process_stream` does not yield back to a multi-threaded executor in tests, but may need adjustment when wired into the full store.

3. **12 new tests are well-structured**: The test suite covers the trait shape, checkpoint serialization, file I/O (save/load/corrupt/missing/nested dirs), and stream processing (fresh, idempotent, skipped events, roundtrip). Good coverage.

## Scope Check

- Files within scope: YES -- `src/projection.rs` (full rewrite) and `src/lib.rs` (module wiring) are both listed in the ticket scope or are necessary for the module to compile.
- Scope creep detected: NO -- The lib.rs changes are from prior tickets (1-8) that restructured the module declarations. The projection-specific changes (`mod projection;` and `pub use projection::Projection;`) were already present in the committed HEAD and are simply retained.
- Unauthorized dependencies added: NO -- All imports (`tokio_stream`, `tonic`, `serde`, `tracing`, `tempfile`) are already in Cargo.toml from prior tickets.

## Risk Assessment

- Regression risk: LOW -- This is a full rewrite of a module that was not compiled (the old code referenced `eventfold::Event` which no longer exists). No downstream consumers exist yet (store.rs is still commented out). The `Projection` trait is re-exported publicly and matches the PRD contract.
- Security concerns: NONE
- Performance concerns: NONE -- Sync file I/O in async context is negligible for checkpoint files. Stream processing is linear in event count.
