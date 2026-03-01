# Tickets for PRD 013: TLS and Bearer Token Authentication

**Source PRD:** docs/prd/013-tls-and-bearer-token-auth.md
**Created:** 2026-02-28
**Total Tickets:** 4
**Estimated Total Complexity:** 7 (S=1 + M=2 + M=2 + S=1 + S=1)

---

### Ticket 1: Add `tls` Feature Flag, `BearerInterceptor` Module, and `mod auth` Declaration

**Description:**
Create the foundational auth infrastructure: add the `tls` Cargo feature, introduce the new
`src/auth.rs` module containing the `BearerInterceptor` interceptor type, and declare it as a
private module in `src/lib.rs`. Nothing else in the codebase changes in this ticket; the
interceptor is not yet wired into `EsClient`.

**Scope:**
- Create: `src/auth.rs`
- Modify: `Cargo.toml` (add `[features]` section)
- Modify: `src/lib.rs` (add `mod auth;`)

**Acceptance Criteria:**
- [ ] `Cargo.toml` gains a `[features]` section with `default = []` and `tls = ["tonic/tls"]`. No
  other dependency versions change.
- [ ] `src/auth.rs` defines a `pub(crate) struct BearerInterceptor` with a single field
  `token: Arc<std::sync::RwLock<String>>` and derives `Clone`.
- [ ] `BearerInterceptor` implements `tonic::service::Interceptor`. The `call` method uses
  `std::sync::RwLock::read().expect("token RwLock poisoned")` (synchronous, not async). If the
  token string is empty, no `authorization` header is added. If non-empty, it inserts
  `"Bearer {token}"` as the `authorization` metadata value. On a parse error it returns
  `tonic::Status::internal("invalid token characters")`.
- [ ] `src/lib.rs` adds `mod auth;` as a private (non-pub) module declaration.
- [ ] `cargo build` succeeds (no features and `--features tls`).
- [ ] Test: construct a `BearerInterceptor { token: Arc::new(RwLock::new("abc".to_string())) }`,
  call `interceptor.call(tonic::Request::new(()))`, assert the returned request has metadata key
  `"authorization"` equal to `"Bearer abc"`.
- [ ] Test: construct a `BearerInterceptor { token: Arc::new(RwLock::new(String::new())) }`,
  call `interceptor.call(tonic::Request::new(()))`, assert the returned request has no
  `"authorization"` metadata key.
- [ ] Test: construct a `BearerInterceptor` with token `"abc"`, call `call()` once (observe
  `"Bearer abc"`), then `*interceptor.token.write().unwrap() = "xyz".to_string()`, call `call()`
  again, assert the returned metadata value is `"Bearer xyz"` (token mutation visible on next
  call).
- [ ] Quality gates pass: `cargo build`, `cargo clippy -- -D warnings`, `cargo fmt --check`,
  `cargo test` (no regressions).

**Dependencies:** None
**Complexity:** S
**Maps to PRD AC:** AC 1, AC 7

---

### Ticket 2: Refactor `EsClient` — `EsClientInner` Enum, `connect_with_token`, Updated RPC Dispatch

**Description:**
Rework `src/client.rs` to support both plain and authenticated transports through an internal
`EsClientInner` enum. Wrap the enum in `Arc` so `Clone` remains cheap. Update the three RPC
methods (`append`, `read_stream`, `subscribe_all_from`) to dispatch over both variants. Add the
public `connect_with_token` constructor and update the test-only `from_inner` helper to wrap in
the `Plain` variant.

**Scope:**
- Modify: `src/client.rs`

**Acceptance Criteria:**
- [ ] Add private type aliases `PlainClient = EventStoreClient<Channel>` and `AuthClient =
  EventStoreClient<tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>>`.
- [ ] Add a private `enum EsClientInner { Plain(PlainClient), Auth(AuthClient) }`.
- [ ] `EsClient` struct field changes from `inner: EventStoreClient<Channel>` to
  `inner: Arc<EsClientInner>`. The `Debug` derive must be replaced with a manual `Debug` impl
  (or `#[derive(Debug)]` removed and replaced with `impl std::fmt::Debug for EsClient`) because
  `EsClientInner` does not derive `Debug`. `Clone` is preserved via `Arc::clone`.
- [ ] `EsClient::connect` builds the `Plain` variant: `Ok(Self { inner: Arc::new(EsClientInner::Plain(inner)) })`.
- [ ] `EsClient::connect_with_token(endpoint: &str, token: Arc<std::sync::RwLock<String>>) ->
  Result<Self, tonic::transport::Error>` connects via `tonic::transport::Endpoint::from_shared`,
  wraps with `EventStoreClient::with_interceptor(channel, BearerInterceptor { token })`, and
  wraps the result in `EsClientInner::Auth`.
- [ ] `EsClient::from_inner` (test-only) wraps its argument in `EsClientInner::Plain` instead of
  assigning directly. All 11 existing call sites in test modules compile unchanged because the
  function signature is identical.
- [ ] Each of the three RPC methods (`append`, `read_stream`, `subscribe_all_from`) matches on
  `self.inner.as_ref()` and calls the same RPC on both `Plain` and `Auth` variants. No RPC
  logic changes.
- [ ] `EsClient` is `pub use`-d from `src/lib.rs` as before; no public API changes.
- [ ] Test: `EsClient::connect_with_token` with a token of `"abc123"` — construct a mock channel
  using `tower::service_fn` that captures request metadata, call `append` (or any RPC), assert
  the captured `authorization` header value equals `"Bearer abc123"`.

  _Implementer note: `tower::service_fn` mock channels in tonic tests require the `tower` crate
  as a dev-dependency. Add `tower = "0.5"` to `[dev-dependencies]` in `Cargo.toml` if not
  already present. Confirm the exact mock-channel pattern against tonic 0.13 docs/examples before
  starting — tonic's `Channel` cannot be constructed directly from a `service_fn`; use
  `tonic::transport::Endpoint::from_static(...).connect_lazy()` + intercept, OR construct a mock
  `Channel` via `tonic::transport::channel::Channel` internal test helpers. If a mock channel
  pattern is not straightforward in tonic 0.13, the test can instead construct
  `BearerInterceptor` directly and verify its `call()` output (which is already done in Ticket 1
  tests) and verify `connect_with_token` returns `Ok(EsClientInner::Auth(_))` using a lazy
  channel that doesn't require a server — see AC in Ticket 3._
- [ ] Test: `EsClient::connect_with_token` with an empty token string — same mock or lazy channel
  approach, verify no `authorization` metadata is present in the resulting client's interceptor
  (via `BearerInterceptor::call` directly or via a captured request).
- [ ] `cargo build` and `cargo test` pass with no regressions.

**Dependencies:** Ticket 1
**Complexity:** M
**Maps to PRD AC:** AC 2, AC 3, AC 4, AC 5

---

### Ticket 3: Add `auth_token` Builder Method to `AggregateStoreBuilder`

**Description:**
Extend `AggregateStoreBuilder` in `src/store.rs` with an optional `auth_token` field and
a corresponding `.auth_token()` builder method. In `open()`, branch on the field to call
`EsClient::connect_with_token` when a token is supplied, falling back to `EsClient::connect`
when not. Add a unit test verifying the correct `EsClientInner` variant is held after building
with a token.

**Scope:**
- Modify: `src/store.rs`

**Acceptance Criteria:**
- [ ] `AggregateStoreBuilder` gains a field `auth_token: Option<Arc<std::sync::RwLock<String>>>`,
  initialized to `None` in `AggregateStoreBuilder::new()`.
- [ ] `pub fn auth_token(mut self, token: Arc<std::sync::RwLock<String>>) -> Self` sets
  `self.auth_token = Some(token)` and returns `self`.
- [ ] The `auth_token` method has a doc comment explaining the shared-token / in-place-refresh
  semantics and the empty-string no-header behaviour.
- [ ] In `AggregateStoreBuilder::open`, the existing `EsClient::connect(endpoint).await?` is
  replaced with:
  ```
  let client = match self.auth_token {
      Some(token) => EsClient::connect_with_token(endpoint, token).await?,
      None        => EsClient::connect(endpoint).await?,
  };
  ```
- [ ] Existing call `AggregateStoreBuilder::new().endpoint("http://...").open().await` compiles
  and behaves identically (no regression); the `None` branch is exercised.
- [ ] Test: construct `AggregateStoreBuilder::new()` without calling `.auth_token()`, confirm
  `auth_token` field is `None` (via a unit test that inspects the builder struct before `open`).
- [ ] Test: construct `AggregateStoreBuilder::new().auth_token(Arc::new(RwLock::new("tok".to_string())))`,
  confirm `auth_token` field is `Some(...)` (pre-`open` struct inspection).
- [ ] Test: using a lazy/no-server channel (same pattern as `mock_store`), call
  `AggregateStoreBuilder::new().endpoint("http://[::1]:1").auth_token(token).open().await`
  (connecting to port 1 will fail with a transport error, so use `connect_lazy` path — see note).

  _Implementer note: `open()` calls `EsClient::connect` which actually attempts connection. The
  existing test `builder_connect_returns_err_when_no_server` shows the pattern. For the `Auth`
  variant check (PRD AC 6), the cleanest approach is: add a `#[cfg(test)]` helper
  `AggregateStoreBuilder::open_with_lazy_client` (or expose `EsClientInner` as `pub(crate)`)
  that constructs the store with a pre-built `EsClient` so tests can bypass the TCP connect.
  Alternatively, test that `connect_with_token` returns `EsClientInner::Auth` from the
  `EsClient` level (Ticket 2 AC already covers this), and here test only that `auth_token()`
  fluently returns `Self` and that `self.auth_token` is `Some`._
- [ ] Test: `matches!(store.client.inner.as_ref(), EsClientInner::Auth(_))` — verifying AC 6 from
  the PRD. This requires `EsClientInner` to be `pub(crate)` and the `inner` field of `EsClient`
  to be accessible from `store.rs` tests (same module as existing `mock_store` helpers which
  already access `crate::client` internals). Implement a `#[cfg(test)]` method
  `EsClient::is_auth() -> bool` that returns `matches!(*self.inner, EsClientInner::Auth(_))` to
  avoid exposing field internals.
- [ ] `cargo build` and `cargo test` pass with no regressions.

**Dependencies:** Ticket 2
**Complexity:** M
**Maps to PRD AC:** AC 2, AC 6

---

### Ticket 4: Verification and Integration Check

**Description:**
Run the full PRD 013 acceptance criteria checklist. Verify all tickets integrate correctly:
build succeeds with and without the `tls` feature, all new and existing tests are green, no
clippy warnings, and code is formatted. Confirm via grep that all 4 PRD-mandated test scenarios
exist in the test suite.

**Scope:**
- No source file changes expected. If any quality gate fails, fix the root cause.

**Acceptance Criteria:**
- [ ] `cargo build` exits with code 0 (no features).
- [ ] `cargo build --features tls` exits with code 0.
- [ ] `cargo test --all-features` exits with code 0; all tests (old + new) pass.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` exits with code 0.
- [ ] `cargo fmt --check` exits with code 0.
- [ ] Grep confirms: `src/auth.rs` contains `BearerInterceptor` struct.
- [ ] Grep confirms: `src/client.rs` contains `EsClientInner` enum, `connect_with_token` fn.
- [ ] Grep confirms: `src/store.rs` contains `auth_token` field and method.
- [ ] Grep confirms: at least 4 new `#[test]` functions exist across `src/auth.rs` and
  `src/client.rs` covering the PRD test scenarios (non-empty token, empty token, token mutation
  visibility, Auth variant check).
- [ ] All PRD acceptance criteria pass (see AC Coverage Matrix below).

**Dependencies:** Tickets 1, 2, 3
**Complexity:** S
**Maps to PRD AC:** AC 1–10

---

## AC Coverage Matrix

| PRD AC # | Description                                                                                           | Covered By Ticket(s)   | Status  |
|----------|-------------------------------------------------------------------------------------------------------|------------------------|---------|
| 1        | `cargo build` (no features) and `cargo build --features tls` both succeed; no code requires `tls`    | Ticket 1, Ticket 4     | Covered |
| 2        | `AggregateStoreBuilder::new().endpoint(...).open()` compiles + behaves identically (no regression)   | Ticket 3, Ticket 4     | Covered |
| 3        | `EsClient::connect(...)` compiles + behaves identically to current version (`Plain` variant)         | Ticket 2, Ticket 4     | Covered |
| 4        | `connect_with_token` with `"abc123"` sends `Authorization: Bearer abc123` on every RPC; unit test    | Ticket 1, Ticket 2     | Covered |
| 5        | `connect_with_token` with empty string sends no `Authorization` header; unit test                    | Ticket 1, Ticket 2     | Covered |
| 6        | Store built with `.auth_token(token)` holds `EsClientInner::Auth` variant; unit test                 | Ticket 3               | Covered |
| 7        | Mutating token after construction causes new value to appear on next RPC call; unit test              | Ticket 1               | Covered |
| 8        | `cargo clippy --all-targets --all-features -- -D warnings` exits 0                                   | Ticket 4               | Covered |
| 9        | `cargo test --all-features` passes; ACs 4, 5, 6, 7 tests present and green                           | Ticket 4               | Covered |
| 10       | `cargo fmt --check` exits 0                                                                           | Ticket 4               | Covered |
