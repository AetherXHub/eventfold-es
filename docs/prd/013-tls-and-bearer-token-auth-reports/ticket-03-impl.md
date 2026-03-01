# Implementation Report: Ticket 3 -- Add `auth_token` Builder Method to `AggregateStoreBuilder`

**Ticket:** 3 - Add `auth_token` Builder Method to `AggregateStoreBuilder`
**Date:** 2026-03-01 12:00
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/store.rs` - Added `auth_token` field to `AggregateStoreBuilder`, builder method with doc comment, branching logic in `open()`, and two unit tests.

## Implementation Notes
- The `auth_token` field uses `Option<Arc<std::sync::RwLock<String>>>` exactly as specified, matching the `BearerInterceptor` token type from `src/auth.rs`.
- The builder method includes a comprehensive doc comment explaining: (1) shared-token semantics via `Arc<RwLock<String>>`, (2) in-place refresh by writing to the lock, (3) empty-string no-header behaviour.
- In `open()`, the existing `EsClient::connect(endpoint).await?` is replaced with a `match` on `self.auth_token` that dispatches to `EsClient::connect_with_token` for `Some(token)` and `EsClient::connect` for `None`.
- Tests verify builder field state directly rather than attempting live gRPC connections, following the ticket's guidance about the cleanest approach.
- The existing `builder_connect_returns_err_when_no_server` test implicitly verifies the `None` branch in `open()` still works correctly.

## Acceptance Criteria
- [x] AC 1: `AggregateStoreBuilder` gains `auth_token: Option<Arc<std::sync::RwLock<String>>>`, initialized to `None` in `new()`.
- [x] AC 2: `pub fn auth_token(mut self, token: Arc<std::sync::RwLock<String>>) -> Self` sets the field and returns `self`.
- [x] AC 3: Doc comment on `auth_token` method explains shared-token/in-place-refresh semantics and empty-string no-header behaviour.
- [x] AC 4: `open()` branches on `self.auth_token` -- `Some(token) => connect_with_token`, `None => connect`.
- [x] AC 5: Existing `AggregateStoreBuilder::new().endpoint("http://...").open().await` compiles and behaves identically (no regression -- verified by `builder_connect_returns_err_when_no_server` test).
- [x] AC 6: Test `builder_without_auth_token_has_none` -- constructs builder without calling `.auth_token()`, confirms field is `None`.
- [x] AC 7: Test `builder_with_auth_token_has_some` -- constructs builder with `.auth_token(Arc::new(RwLock::new("tok")))`, confirms field is `Some(...)`.
- [x] AC 8: Auth client test via `is_auth()` -- deferred to builder field-level verification per ticket guidance. The `connect_with_token` -> Auth variant path is already tested in Ticket 2's `connect_with_token_creates_auth_variant` test.
- [x] AC 9: `cargo build` and `cargo test` pass with no regressions (139 unit tests + 7 doc-tests).
- [x] AC 10: `cargo clippy -- -D warnings` and `cargo fmt --check` pass.

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (139 tests + 7 doc-tests, 0 failures)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt --check` -- no differences)
- New tests added:
  - `store::tests::builder_without_auth_token_has_none` in `src/store.rs`
  - `store::tests::builder_with_auth_token_has_some` in `src/store.rs`

## Concerns / Blockers
- None
