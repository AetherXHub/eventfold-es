# Ticket 1 Implementation Report

**Ticket:** Add `tls` feature flag, `BearerInterceptor` module, `mod auth` declaration
**Status:** COMPLETE

## Changes

- **Created:** `src/auth.rs` -- `BearerInterceptor` struct with `Interceptor` impl, 3 unit tests
- **Modified:** `Cargo.toml` -- Added `[features]` section with `default = []` and `tls = ["tonic/tls-native-roots"]`
- **Modified:** `src/lib.rs` -- Added `mod auth;` declaration

## Notes

- PRD specified `tls = ["tonic/tls"]` but tonic 0.13 does not have a `tls` feature. The correct feature is `tls-native-roots` which uses platform native CA roots. This is the right choice for Let's Encrypt certs behind Caddy.
- `BearerInterceptor` has expected dead_code warning since it's not wired into `EsClient` yet (Ticket 2).

## Acceptance Criteria

- [x] `Cargo.toml` gains `[features]` section with `default = []` and `tls = ["tonic/tls-native-roots"]`
- [x] `src/auth.rs` defines `pub(crate) struct BearerInterceptor` with `token: Arc<RwLock<String>>`, derives Clone
- [x] `BearerInterceptor` implements `tonic::service::Interceptor` with synchronous RwLock read
- [x] `src/lib.rs` adds `mod auth;`
- [x] `cargo build` succeeds (no features and `--features tls`)
- [x] Test: non-empty token inserts Bearer header
- [x] Test: empty token omits authorization header
- [x] Test: token mutation visible on next call
- [x] Quality gates pass: 131 tests, build clean, fmt clean

## Test Results

- 131 unit tests + 7 doc-tests pass
- `cargo build` and `cargo build --features tls` succeed
