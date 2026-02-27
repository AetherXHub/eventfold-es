# Implementation Report: Ticket 1 -- Cargo.toml, build.rs, and proto codegen

**Ticket:** 1 - Cargo.toml, build.rs, and proto codegen
**Date:** 2026-02-27 12:50
**Status:** COMPLETE

---

## Files Changed

### Created
- `build.rs` - Proto codegen build script that compiles `../eventfold-db/proto/eventfold.proto` via `tonic_build::compile_protos` and emits `cargo:rerun-if-changed`

### Modified
- `Cargo.toml` - Removed `eventfold = "0.2.0"`; added `tonic`, `prost`, `bytes`, `tokio-stream`, `uuid` (with v4+v5 features); added `[build-dependencies]` with `tonic-build = "0.13"`; added `autoexamples = false`
- `src/lib.rs` - Commented out all modules that depend on `eventfold`; added `pub mod proto` with `tonic::include_proto!("eventfold")`; added 7 unit tests verifying generated proto types are accessible

## Implementation Notes
- All modules (`actor`, `aggregate`, `command`, `error`, `process_manager`, `projection`, `storage`, `store`) were commented out in `src/lib.rs` because they all transitively depend on the removed `eventfold` crate. Each will be rewritten in subsequent tickets.
- `autoexamples = false` was added to `Cargo.toml` because `examples/counter.rs` imports types (`Aggregate`, `AggregateStore`, `Projection`, etc.) that are no longer in the public API. This is outside the stated file scope but necessary for `cargo build` / `cargo test` to succeed.
- The generated proto types use `Vec<u8>` for `bytes` proto fields (prost default), matching eventfold-db's own build.rs configuration. The `bytes` crate is still added as a direct dependency per the AC, as it will be needed by the gRPC client wrapper in later tickets.
- `tonic::include_proto!("eventfold")` is the idiomatic equivalent of `include!(concat!(env!("OUT_DIR"), "/eventfold.rs"))` -- it expands to the same thing.

## Acceptance Criteria
- [x] AC 1: Cargo.toml removes `eventfold = "0.2.0"` and adds: `tonic = "0.13"`, `prost = "0.13"`, `bytes = "1"`, `tokio-stream = "0.1"`, `uuid = { version = "1", features = ["v4", "v5"] }` -- all present in `[dependencies]`
- [x] AC 2: Cargo.toml adds `[build-dependencies]` section with `tonic-build = "0.13"` -- present at line 29-30
- [x] AC 3: build.rs calls `tonic_build::compile_protos("../eventfold-db/proto/eventfold.proto")` and propagates errors -- uses `?` operator with `Result<(), Box<dyn std::error::Error>>`
- [x] AC 4: build.rs emits `cargo:rerun-if-changed=../eventfold-db/proto/eventfold.proto` -- present at line 3
- [x] AC 5: `cargo build` compiles cleanly; proto codegen succeeds; generated module accessible via `tonic::include_proto!("eventfold")` -- verified; 7 tests exercise all major proto types
- [x] AC 6: `cargo clippy -- -D warnings` and `cargo fmt --check` pass -- both pass with zero warnings

## Test Results
- Lint: PASS (`cargo clippy -- -D warnings` -- zero warnings)
- Tests: PASS (7 passed, 0 failed)
- Build: PASS (`cargo build` -- zero warnings)
- Format: PASS (`cargo fmt --check` -- no diffs)
- New tests added: 7 tests in `src/lib.rs` (`tests` module):
  - `proto_proposed_event_is_accessible`
  - `proto_recorded_event_is_accessible`
  - `proto_append_request_is_accessible`
  - `proto_expected_version_exact_variant`
  - `proto_subscribe_response_variants`
  - `proto_list_streams_response_is_accessible`
  - `proto_grpc_client_type_exists`

## Concerns / Blockers
- `examples/counter.rs` was not modified (out of scope), but `autoexamples = false` was added to `Cargo.toml` to prevent it from failing the build. A later ticket should either rewrite or remove the example.
- The `#![warn(missing_docs)]` attribute from the previous `src/lib.rs` was not present in the original file and is not added here. The `proto` module's generated code would trigger missing_docs warnings if it were enabled. Subsequent tickets should consider whether to add `#[allow(missing_docs)]` on the proto module.
- The commented-out module declarations in `src/lib.rs` serve as a roadmap for subsequent tickets. Each module file still exists on disk but is not compiled.
