# Code Review: Ticket 1 -- LiveConfig struct and builder integration

**Ticket:** 1 -- LiveConfig struct and builder integration
**Impl Report:** docs/prd/010-live-subscriptions-reports/ticket-01-impl.md
**Date:** 2026-02-27 16:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| AC 10 | `LiveConfig::default()` returns checkpoint_interval=5s, reconnect_base_delay=1s, reconnect_max_delay=30s | Met | Verified in `src/live.rs` lines 58-64. Unit test `live::tests::live_config_default_values` (line 72) asserts all three values. Doc-test (line 19) also exercises the struct. |
| AC 11 | `AggregateStoreBuilder::new().live_config(custom).open()` stores the custom config | Met | Builder method at `src/store.rs` line 581-583. Test `store::tests::builder_stores_custom_live_config` (line 706) constructs a custom config with non-default values (42s, 500ms, 60s) and asserts all three fields. Uses `mock_store_with_live_config` which directly constructs the store (bypasses gRPC connect), achieving the same field propagation. Test `store::tests::builder_default_live_config` (line 731) verifies default propagation through `mock_store()`. |
| AC 14 (import) | `LiveConfig` is publicly importable from `eventfold_es::LiveConfig` | Met | `pub use live::LiveConfig;` in `src/lib.rs` line 32. Doc-test uses `use eventfold_es::LiveConfig;` and compiles+runs successfully. |
| AC 14 (docs) | `LiveConfig` has doc comments on struct and all fields | Met | Struct doc comment at `src/live.rs` lines 8-29 (includes cross-reference to builder method and a working example). Field docs: `checkpoint_interval` (lines 32-39), `reconnect_base_delay` (lines 42-49), `reconnect_max_delay` (lines 51-54). All include default values. |
| - | `cargo test` passes with no new failures | Met | 96 unit tests + 6 doc-tests, all pass. Verified independently. |
| - | `cargo clippy -- -D warnings` passes | Met | Zero warnings. Verified independently. |

## Issues Found

### Critical (must fix before merge)
- None.

### Major (should fix, risk of downstream problems)
- None.

### Minor (nice to fix, not blocking)
- **Missing `PartialEq` derive on `LiveConfig`** (`src/live.rs` line 30): The struct derives `Debug, Clone` but not `PartialEq`. Per global CLAUDE.md: "derive common traits: `Debug`, `Clone`, `PartialEq` where appropriate." All fields are `Duration` which implements `PartialEq`. The tests work around this by comparing fields individually. Deriving `PartialEq` would simplify assertions and is consistent with the convention. Adding `Eq` would also be appropriate since `Duration: Eq`.

## Suggestions (non-blocking)
- The `builder_stores_custom_live_config` and `builder_default_live_config` tests are `#[tokio::test]` but contain no async operations. The impl report explains this is because `connect_lazy()` requires a Tokio runtime context. This is a pre-existing pattern in the codebase (the existing `get_twice_returns_cached_alive_handles` test is also `#[tokio::test]` for the same reason), so this is fine.
- The doc comment for `live_config` builder method references `AggregateStore::start_live` which does not exist yet. The intra-doc link is written as `[AggregateStore::start_live](AggregateStore)` which resolves to `AggregateStore` (not broken), but the text implies a method that will only exist after a follow-on ticket. This is acceptable for a WIP feature being built across tickets.

## Scope Check
- Files within scope: YES -- `src/live.rs` (created), `src/lib.rs` (modified), `src/store.rs` (modified) are exactly the files listed in the ticket scope.
- Scope creep detected: NO -- Changes are minimal and focused. The `mock_store_with_live_config` helper is a clean refactoring of `mock_store` to support the new field without duplicating construction logic.
- Unauthorized dependencies added: NO -- No new dependencies in `Cargo.toml`.

## Risk Assessment
- Regression risk: LOW -- The `#[allow(dead_code)]` field on `AggregateStore` is inert; it cannot affect existing behavior. The `mock_store()` refactoring delegates to `mock_store_with_live_config()` with `LiveConfig::default()`, preserving identical behavior for existing tests.
- Security concerns: NONE
- Performance concerns: NONE -- `LiveConfig` is a small stack-allocated struct with three `Duration` fields. Cloning is trivial.
