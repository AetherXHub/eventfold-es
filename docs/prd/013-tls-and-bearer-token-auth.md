# PRD 013: TLS and Bearer Token Authentication

**Status:** DRAFT
**Created:** 2026-02-28
**Author:** PRD Writer Agent
**GitHub Issue:** #7

---

## Problem Statement

`eventfold-es` currently connects to `eventfold-db` over plaintext HTTP/2 with
no authentication, making it unsuitable for production deployments where the
server is hosted on a remote machine (e.g. Hetzner behind a Caddy reverse
proxy) that requires TLS and JWT-based Bearer authentication. Applications
cannot adopt `eventfold-db` in production until the client supports encrypted
transport and per-request token injection that is compatible with background
token refresh without reconnecting.

## Goals

- Enable opt-in TLS transport by exposing tonic's TLS support behind a Cargo
  feature flag so existing plain-HTTP setups require no changes.
- Allow callers to supply an `Arc<RwLock<String>>` auth token that is read on
  every outgoing RPC, supporting in-place token refresh without reconnection.
- Expose the token mechanism on both `AggregateStoreBuilder` and `EsClient` so
  both entry points work identically.
- Preserve full backwards compatibility: zero existing call sites change.

## Non-Goals

- Mutual TLS (mTLS) / client certificate authentication.
- Custom CA certificate configuration (can be a follow-up PRD).
- OAuth2 / OIDC token acquisition or refresh logic — callers supply a pre-obtained
  token string; refresh is entirely the application's responsibility.
- Changing `eventfold-db`'s server-side JWT validation implementation.
- Per-request token parameters on individual RPC methods; the interceptor is
  configured once at channel construction time.
- Token expiry detection or automatic reconnection triggered by a 401/UNAUTHENTICATED
  response; the existing exponential-backoff reconnection in `src/live.rs`
  (`run_live_loop`) handles auth-rejected streams the same way it handles any
  stream error.

## User Stories

- As a Tauri desktop app author, I want to connect to `eventfold-db` over TLS
  by passing an `https://` endpoint URL, so that my traffic is encrypted without
  writing any custom channel setup code.
- As a Tauri desktop app author, I want to provide a `Arc<RwLock<String>>` token
  to `AggregateStoreBuilder` so that every gRPC call includes an `Authorization:
  Bearer <token>` header and my background refresh loop can update the token
  in-place without restarting the store.
- As a library integrator using `EsClient` directly, I want a
  `connect_with_token` constructor that mirrors the token-injection behaviour of
  the builder, so that I can use the lower-level client in the same authenticated
  way.
- As an existing `eventfold-es` user on a local dev setup, I want my existing
  code using `AggregateStoreBuilder::new().endpoint("http://...")` to keep
  compiling and running without any changes, so that upgrading the library does
  not break my project.

## Technical Approach

### Feature flag

Add a `tls` Cargo feature that gates tonic's TLS transport:

```toml
# Cargo.toml
[features]
default = []
tls = ["tonic/tls"]
```

When a caller specifies an `https://` endpoint, tonic's `Channel` builder
automatically negotiates TLS if the URI scheme is `https`. No explicit
`tls_config()` call is required; the feature flag is only needed to compile
tonic's TLS support into the binary. No code in `eventfold-es` needs to
branch on the feature flag at runtime.

### New module: `src/auth.rs`

Introduce a small, self-contained module that owns the interceptor type:

```rust
/// gRPC interceptor that injects a Bearer token from a shared, refreshable
/// string.
///
/// The token is read from the `RwLock` on every intercepted request using
/// `blocking_read()` because tonic interceptors are synchronous. If the token
/// string is empty, no `authorization` header is added.
#[derive(Clone)]
pub(crate) struct BearerInterceptor {
    token: Arc<std::sync::RwLock<String>>,
}

impl tonic::service::Interceptor for BearerInterceptor {
    fn call(
        &mut self,
        mut req: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        let token = self.token.read().expect("token RwLock poisoned");
        if !token.is_empty() {
            let value = format!("Bearer {token}")
                .parse::<tonic::metadata::MetadataValue<_>>()
                .map_err(|_| tonic::Status::internal("invalid token characters"))?;
            req.metadata_mut().insert("authorization", value);
        }
        Ok(req)
    }
}
```

Note: the `RwLock` used here is `std::sync::RwLock` (not `tokio::sync::RwLock`)
because tonic interceptors are called synchronously. The public API accepts
`Arc<std::sync::RwLock<String>>` throughout.

`BearerInterceptor` is `pub(crate)` — it is an implementation detail of
`EsClient` and `AggregateStoreBuilder`.

### Changes to `src/client.rs`

`EsClient` currently holds `EventStoreClient<Channel>`. To accommodate the
interceptor the inner type becomes a type alias:

```rust
// Type alias used when no interceptor is configured (current behaviour).
type PlainClient = EventStoreClient<Channel>;

// Type alias used when a BearerInterceptor is attached.
type AuthClient = EventStoreClient<
    tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>
>;
```

To avoid duplicating all RPC methods for two inner variants, introduce a
private `EsClientInner` enum:

```rust
enum EsClientInner {
    Plain(PlainClient),
    Auth(AuthClient),
}
```

`EsClient` becomes:

```rust
#[derive(Clone)]
pub struct EsClient {
    inner: Arc<EsClientInner>,
}
```

Wrapping in `Arc` preserves the cheap-clone property (`Arc<EsClientInner>`
clones as a pointer copy regardless of which variant is held).

Each RPC method (`append`, `read_stream`, `subscribe_all_from`) dispatches on
`self.inner` via a private helper that returns a `&mut dyn` or simply by
`match`-ing both variants. Since the generated `EventStoreClient<T>` methods
require `T: tonic::client::GrpcService<...>`, the cleanest approach is to
match on the enum and call the method on each branch — three small `match`
blocks, one per RPC.

Add the new constructor:

```rust
/// Connect to an `eventfold-db` server with a refreshable Bearer token.
///
/// # Arguments
///
/// * `endpoint` - The URI of the gRPC server. Use `https://` with the `tls`
///   feature to enable encrypted transport.
/// * `token` - Shared token string read on every RPC. An empty string sends
///   no `authorization` header.
///
/// # Errors
///
/// Returns [`tonic::transport::Error`] if the channel cannot be established.
pub async fn connect_with_token(
    endpoint: &str,
    token: Arc<std::sync::RwLock<String>>,
) -> Result<Self, tonic::transport::Error> {
    let channel = tonic::transport::Endpoint::from_shared(endpoint.to_string())?
        .connect()
        .await?;
    let interceptor = BearerInterceptor { token };
    let inner = EventStoreClient::with_interceptor(channel, interceptor);
    Ok(Self { inner: Arc::new(EsClientInner::Auth(inner)) })
}
```

`EsClient::connect` is unchanged and continues to build a `Plain` variant.

The existing `#[cfg(test)] pub(crate) fn from_inner(inner: EventStoreClient<Channel>)` constructor
is updated to wrap its argument in `EsClientInner::Plain`.

### Changes to `src/store.rs` — `AggregateStoreBuilder`

Add a single optional field:

```rust
pub struct AggregateStoreBuilder {
    // ... existing fields ...
    auth_token: Option<Arc<std::sync::RwLock<String>>>,
}
```

Add the builder method:

```rust
/// Attach a refreshable Bearer token to all outgoing gRPC requests.
///
/// The token is read from the `RwLock` on every request, so a background
/// task can update it in-place without reconnecting. An empty string sends
/// no `authorization` header.
///
/// # Arguments
///
/// * `token` - Shared, refreshable token string.
pub fn auth_token(mut self, token: Arc<std::sync::RwLock<String>>) -> Self {
    self.auth_token = Some(token);
    self
}
```

In `AggregateStoreBuilder::open`, branch on `auth_token`:

```rust
let client = match self.auth_token {
    Some(token) => EsClient::connect_with_token(endpoint, token).await?,
    None => EsClient::connect(endpoint).await?,
};
```

### Changes to `src/lib.rs`

Re-export `std::sync::RwLock` is not needed; callers use it directly from std.
No new public items need to be added to `lib.rs` beyond ensuring
`AggregateStoreBuilder` (already exported) gains the new method.

### File-change table

| File | Change |
|------|--------|
| `Cargo.toml` | Add `[features]` section: `default = []`, `tls = ["tonic/tls"]` |
| `src/auth.rs` | New file: `BearerInterceptor` struct + `Interceptor` impl |
| `src/client.rs` | Add `EsClientInner` enum, wrap `EsClient.inner` in `Arc<EsClientInner>`, add `connect_with_token`, update RPC dispatch, update `from_inner` |
| `src/store.rs` | Add `auth_token` field to `AggregateStoreBuilder`, add `auth_token()` builder method, branch in `open()` |
| `src/lib.rs` | Add `mod auth;` (private) |

### Live subscription compatibility

The live subscription loop in `src/live.rs` calls
`store.client.clone().subscribe_all_from(from_position)` inside its reconnect
loop. Because `EsClient::clone` copies the `Arc<EsClientInner>` (pointing to the
same `BearerInterceptor`), the refreshed token is visible on every reconnect
attempt without any changes to `live.rs`. Auth-rejected streams produce a
`tonic::Status` error which the existing `StreamOutcome::Error` branch handles
by applying exponential backoff and reconnecting — at which point the
interceptor reads the (now refreshed) token from the `RwLock`.

## Acceptance Criteria

1. `cargo build` with no features succeeds, and `cargo build --features tls`
   also succeeds; no existing code in the crate requires the `tls` feature to
   compile.
2. `AggregateStoreBuilder::new().endpoint("http://127.0.0.1:2113").open().await`
   compiles and behaves identically to the current version (no regression for
   callers that do not call `.auth_token()`).
3. `EsClient::connect("http://127.0.0.1:2113").await` compiles and behaves
   identically to the current version (the `Plain` variant code path is
   unchanged).
4. An `EsClient` constructed with `connect_with_token(endpoint, token)` where
   `token` contains `"abc123"` sends an `Authorization: Bearer abc123` header on
   every RPC; this is verified in a unit test using a `tower::service_fn` mock
   channel that inspects request metadata.
5. An `EsClient` constructed with `connect_with_token(endpoint, token)` where
   `token` is an empty string sends no `Authorization` header; verified in a unit
   test using a mock channel.
6. An `AggregateStore` built with `.auth_token(token)` delegates to
   `EsClient::connect_with_token` and the resulting store's `client` field
   holds an `EsClientInner::Auth` variant; verified by a unit test that
   constructs the builder with a mock endpoint and checks
   `matches!(store.client.inner.as_ref(), EsClientInner::Auth(_))`.
7. Mutating the `String` inside the `RwLock` after `connect_with_token` is
   called causes the new value to appear in the `authorization` header on the
   next RPC; verified in a unit test by constructing a `BearerInterceptor`
   directly, calling `call()` once (observing token A), updating the string,
   and calling `call()` again (observing token B).
8. `cargo clippy --all-targets --all-features -- -D warnings` exits with code 0.
9. `cargo test --all-features` passes with all existing tests green and the
   new tests described in ACs 4, 5, 6, and 7 present and passing.
10. `cargo fmt --check` exits with code 0.

## Open Questions

- When `tonic/tls` is enabled and the caller passes an `https://` endpoint, tonic
  uses the platform's native TLS roots. If `eventfold-db` is behind Caddy with a
  Let's Encrypt certificate this should work without configuration; however, if
  the server uses a self-signed certificate the connection will fail at the TLS
  handshake. Custom CA configuration is explicitly out of scope for this PRD —
  document this limitation in the `tls` feature docs.
- The `BearerInterceptor` uses `std::sync::RwLock::blocking_read()` (i.e.
  `read().expect(...)`). If the `RwLock` is ever poisoned (writer panicked while
  holding the write lock), the interceptor panics. This is the correct choice for
  a library invariant violation; document it in the doc comment.

## Dependencies

- `tonic` 0.13 already in `Cargo.toml`; enabling `tonic/tls` requires no version
  bump, only the feature flag addition.
- `tonic::service::Interceptor` and `tonic::service::interceptor::InterceptedService`
  are stable tonic 0.13 public API — no additional crate dependencies needed.
- PRD 011 (extract `eventfold-proto`) is independent; this PRD can be implemented
  before or after PRD 011 without conflict.
