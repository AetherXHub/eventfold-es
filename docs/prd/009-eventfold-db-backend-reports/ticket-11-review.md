# Code Review: Ticket 11 -- src/store.rs: rewrite to AggregateStoreBuilder with endpoint + base_dir

**Ticket:** 11 -- src/store.rs: rewrite to AggregateStoreBuilder with endpoint + base_dir
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-11-impl.md
**Date:** 2026-02-27 16:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | AggregateStoreBuilder has methods: new(), endpoint(), base_dir(), aggregate_type::<A>(), projection::<P>(), process_manager::<PM>(), idle_timeout(), async open() | Met | All 8 methods verified at lines 418, 439, 455, 473, 498, 523, 555, 575 of `src/store.rs`. `open()` returns `Result<AggregateStore, tonic::transport::Error>`. |
| 2 | AggregateStore holds client, base_dir, caches; no StreamLayout | Met | Struct at lines 88-96: `client: EsClient`, `base_dir: PathBuf`, `cache`, `projections`, `process_managers`, `dispatchers`, `idle_timeout`. No `StreamLayout` field. |
| 3 | get::<A>(id) uses stream_uuid + spawn_actor_with_config | Met | Lines 127-166: derives `stream_id` via `stream_uuid(A::AGGREGATE_TYPE, instance_id)` (done inside `spawn_actor_with_config`), spawns with `spawn_actor_with_config::<A>(id, self.client.clone(), &self.base_dir, config)`. Cache check with read lock, then spawn on miss. |
| 4 | inject_event::<A>(id, proposed, opts) calls client.append(Any); triggers PM if configured | Met | Lines 185-203: derives `stream_id`, calls `client.append(stream_id, ExpectedVersionArg::Any, vec![proposed])`, conditionally calls `self.run_process_managers()`. |
| 5 | projection::<P>() catches up and returns state | Met | Lines 218-232: looks up runner in projection map, locks mutex, calls `runner.catch_up().await`, returns `runner.state().clone()`. Now async (documented breaking change). |
| 6 | list_streams and read_events removed | Met | Grep confirms neither method exists in `src/store.rs`. |
| 7 | examples/counter.rs updated | Met | Uses `AggregateStoreBuilder::new().endpoint(...).base_dir(...)`, new `Projection::apply(&StoredEvent)` signature. No `subscriptions()`, no `CommandBus`, no `eventfold::Event`. |
| 8 | Test: open returns Err on unreachable endpoint | Met | Test `builder_connect_returns_err_when_no_server` at line 660 connects to `http://127.0.0.1:2113` and asserts `result.is_err()`. |
| 9 | Test: get called twice returns alive handles (cache hit) | Met | Test `get_twice_returns_cached_alive_handles` at line 672 pre-populates cache with a handle backed by a live mpsc channel, calls `get` twice, asserts both `is_alive()`. |
| 10 | Quality gates pass | Met | Verified: `cargo fmt --check` (clean), `cargo build` (0 warnings), `cargo clippy -- -D warnings` (clean), `cargo test` (93 unit + 5 doc-tests, 0 failures). |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

1. **`open()` panics on projection/PM initialization failure** (`src/store.rs` lines 595, 603). The `.expect("projection initialization should not fail")` and `.expect("process manager initialization should not fail")` calls will panic if checkpoint file I/O fails. This is library code; panicking on recoverable I/O errors violates the CLAUDE.md convention ("use `.expect()` only for invariant violations"). The old `open()` (which returned `io::Result`) propagated these errors correctly. The constraint is that `tonic::transport::Error` is opaque and cannot wrap `io::Error`, so the return type would need to change to a custom error enum (e.g., `enum OpenError { Transport(tonic::transport::Error), Io(io::Error) }`). This is acknowledged in the impl report as a known limitation. While unlikely to trigger in practice (only fails on filesystem corruption or permissions issues during startup), it could cause hard-to-diagnose panics in production. Deferring to a future ticket is acceptable given the constraint, but the panic should be documented in the `open()` doc comment as a known limitation.

### Minor (nice to fix, not blocking)

1. **Stale `#![allow(dead_code)]` in `src/actor.rs` line 3** and comment "This module is not yet consumed by the store module" -- the store module now imports from actor. The attribute is now unnecessary (the items are used). Same applies to `src/process_manager.rs` line 3, `src/projection.rs` line 3, `src/snapshot.rs` line 3, and `src/storage.rs` line 3 -- these may still have genuinely dead items, but the comments reference "until the store module is rewritten" which is now done. Cleaning these up is cross-module work that can be done in Ticket 12 or a follow-up.

2. **Test `builder_connect_returns_err_when_no_server` uses port 2113** (`src/store.rs` line 663). The ticket AC suggests using "a random high port that is guaranteed unbound, e.g. port 1". Port 2113 is the eventfold-db default and would cause a false positive if a server happened to be running. Using `http://127.0.0.1:1` or a random ephemeral port would be more robust.

3. **`DispatchError` doc comments still reference `CommandBus`** (`src/error.rs` lines 76, 87-88). `CommandBus` is no longer exported from `src/lib.rs`, making these doc links broken. This is pre-existing (from the old lib.rs which did export CommandBus) and not caused by this ticket, but it's now a dead link.

## Suggestions (non-blocking)

- Consider adding a `# Panics` section to the `open()` doc comment documenting the panic on projection/PM initialization I/O failure, per Rust API guidelines for functions that can panic.
- The `get()` method acquires the write lock twice (once to remove, once to insert) with an async gap in between where another caller could also spawn. The old code had this same pattern so it's not a regression, but a future improvement could hold the write lock through the spawn to avoid duplicate actor spawns for the same key.
- The `Default` impl for `AggregateStoreBuilder` (line 626) is good practice for the builder pattern.

## Scope Check

- Files within scope: YES (src/store.rs, src/lib.rs, examples/counter.rs are all listed in Ticket 11 scope)
- Scope creep detected: MINOR
  - `Cargo.toml`: Changed `autoexamples = false` to `autoexamples = true`. This is a reasonable change to ensure the counter example compiles as a build check. Minimal impact.
  - `src/client.rs`: Added `#[cfg(test)] pub(crate) fn from_inner()` (6 lines). Test-only, zero runtime impact.
  - `src/actor.rs`: Added `#[cfg(test)] pub(crate) fn from_sender()` (5 lines). Test-only, zero runtime impact.
  - These out-of-scope changes are justified: they enable the AC 9 unit test (cache hit verification) without requiring a live gRPC server. The implementer flagged these proactively in the impl report.
- Unauthorized dependencies added: NO

## Risk Assessment

- Regression risk: LOW -- This is a complete rewrite of `src/store.rs`, but the old store was tightly coupled to the now-removed `eventfold` crate and `StreamLayout`. The new store compiles, all 93 unit tests pass, and the example compiles. The breaking API changes (async `projection()`, `AggregateStoreBuilder` instead of `AggregateStore::open(path)`) are documented and expected per the PRD.
- Security concerns: NONE
- Performance concerns: NONE -- The `tokio::sync::Mutex` for projections (replacing `std::sync::Mutex`) is correct since `catch_up()` is now async. The `tokio::sync::RwLock` for the handle cache allows concurrent reads. The `Arc<DispatcherMap>` for dispatchers is immutable after construction, so no lock needed.
