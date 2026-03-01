# Implementation Report: Ticket 2 -- Refactor `EsClient` -- `EsClientInner` Enum, `connect_with_token`, Updated RPC Dispatch

**Ticket:** 2 - Refactor `EsClient` -- `EsClientInner` Enum, `connect_with_token`, Updated RPC Dispatch
**Date:** 2026-03-01 19:30
**Status:** COMPLETE

---

## Files Changed

### Created
- None

### Modified
- `src/client.rs` - Refactored `EsClient` to support both plain and authenticated transports via `EsClientInner` enum wrapped in `Arc`. Added `connect_with_token` constructor, `is_auth` test helper, and updated all three RPC methods to dispatch over both variants.
- `src/auth.rs` - Formatting only (auto-applied by `cargo fmt` to fix pre-existing formatting from Ticket 1).
- `src/lib.rs` - Formatting only (auto-applied by `cargo fmt` to fix pre-existing module declaration ordering from Ticket 1).

## Implementation Notes
- **Clone via Arc**: `EsClient`'s inner field changed from `EventStoreClient<Channel>` to `Arc<EsClientInner>`, making clone O(1) via `Arc::clone`.
- **RPC dispatch via clone-per-call**: Since tonic 0.13's generated methods take `&mut self`, each RPC method clones the inner `EventStoreClient` variant before calling. This is cheap because `EventStoreClient` wraps `tonic::client::Grpc<T>` which itself wraps the channel (an Arc'd hyper connection pool). This avoids interior mutability while keeping `EsClient` cheaply cloneable.
- **Manual Debug impl**: `EsClientInner` does not derive `Debug` (the intercepted service type doesn't implement it), so `EsClient` uses a manual `Debug` impl that shows the transport variant name ("Plain" or "Auth").
- **Test pattern**: Tests that construct lazy channels (`connect_lazy()`) use `#[tokio::test]` because the hyper runtime requires a tokio context even for lazy channel creation. The `mock_auth_client` test helper constructs an `Auth` variant with a lazy channel to verify variant detection without a running server.
- **BearerInterceptor tests in client module**: The ticket's test ACs for `connect_with_token` with non-empty and empty tokens are satisfied by testing through `BearerInterceptor::call` directly (the same interceptor wired into the `Auth` variant), following the ticket's guidance for when mock channel patterns are not straightforward.
- **Formatting of auth.rs and lib.rs**: `cargo fmt` corrected pre-existing formatting issues in these files from Ticket 1. These changes are cosmetic only (whitespace/line-wrapping) and do not alter behavior.

## Acceptance Criteria
- [x] AC 1: Private type aliases `PlainClient` and `AuthClient` added (lines 17-22)
- [x] AC 2: Private `enum EsClientInner { Plain(PlainClient), Auth(AuthClient) }` added (lines 24-30)
- [x] AC 3: `EsClient` struct field is `inner: Arc<EsClientInner>`, manual `Debug` impl replaces derive, `Clone` preserved via `Arc` (lines 108-123)
- [x] AC 4: `EsClient::connect` builds the `Plain` variant (lines 142-147)
- [x] AC 5: `EsClient::connect_with_token` connects via `Endpoint::from_shared`, wraps with `with_interceptor`, wraps in `Auth` variant (lines 168-180)
- [x] AC 6: `EsClient::from_inner` (test-only) wraps in `EsClientInner::Plain` -- signature unchanged, all 11 existing call sites compile (lines 186-190)
- [x] AC 7: All three RPC methods (`append`, `read_stream`, `subscribe_all_from`) match on `self.inner.as_ref()` and clone the appropriate variant for the RPC call (lines 237-239, 272-274, 309-311)
- [x] AC 8: `#[cfg(test)] pub(crate) fn is_auth(&self) -> bool` added (lines 195-198)
- [x] AC 9: `cargo build` and `cargo test` pass -- 137 unit tests + 7 doc-tests, zero failures
- [x] AC 10: `cargo clippy -- -D warnings` and `cargo fmt --check` pass

## Test Results
- Lint: PASS (`cargo clippy --all-targets -- -D warnings`)
- Tests: PASS (137 unit tests + 7 doc-tests, 0 failures)
- Build: PASS (`cargo build`, zero warnings)
- Format: PASS (`cargo fmt --check`)
- New tests added:
  - `src/client.rs::tests::from_inner_creates_plain_variant` - Verifies `from_inner` creates Plain variant
  - `src/client.rs::tests::connect_with_token_creates_auth_variant` - Verifies Auth variant construction
  - `src/client.rs::tests::connect_with_token_non_empty_token_injects_bearer_header` - Verifies "abc123" token produces "Bearer abc123" header
  - `src/client.rs::tests::connect_with_token_empty_token_omits_authorization` - Verifies empty token produces no Authorization header
  - `src/client.rs::tests::es_client_debug_shows_transport_variant` - Verifies manual Debug impl shows variant name
  - `src/client.rs::tests::es_client_clone_is_cheap` - Verifies Arc-based clone shares the same allocation

## Concerns / Blockers
- `src/auth.rs` and `src/lib.rs` had pre-existing formatting issues from Ticket 1 that `cargo fmt` corrected. These are formatting-only changes (whitespace/line-wrapping) and do not affect behavior. Noted here since they are outside the stated scope of `src/client.rs`.
