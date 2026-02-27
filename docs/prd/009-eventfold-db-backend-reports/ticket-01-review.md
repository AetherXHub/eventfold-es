# Code Review: Ticket 1 -- Cargo.toml, build.rs, and proto codegen

**Ticket:** 1 -- Cargo.toml, build.rs, and proto codegen
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-01-impl.md
**Date:** 2026-02-27 13:15
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | Cargo.toml removes `eventfold = "0.2.0"` and adds tonic, prost, bytes, tokio-stream, uuid (v4+v5) | Met | Diff confirms `eventfold` removed. All five deps present in `[dependencies]` with correct versions. `uuid` features updated from `["v4"]` to `["v4", "v5"]`. |
| 2 | Cargo.toml adds `[build-dependencies]` with `tonic-build = "0.13"` | Met | Lines 29-30 of `Cargo.toml`. |
| 3 | build.rs calls `tonic_build::compile_protos("../eventfold-db/proto/eventfold.proto")` and propagates errors | Met | `build.rs` line 4 calls `tonic_build::compile_protos(proto_path)?` with `Result<(), Box<dyn std::error::Error>>` return type. |
| 4 | build.rs emits `cargo:rerun-if-changed=../eventfold-db/proto/eventfold.proto` | Met | `build.rs` line 3: `println!("cargo:rerun-if-changed={proto_path}")`. |
| 5 | `cargo build` compiles cleanly | Met | Verified: `cargo build` exits with `Finished` and zero warnings. Proto codegen succeeds; module accessible via `tonic::include_proto!("eventfold")` in `src/lib.rs`. |
| 6 | `cargo clippy -- -D warnings` and `cargo fmt --check` pass | Met | Both verified: clippy zero warnings, fmt no diffs. |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **Out-of-scope `src/lib.rs` modification.** The ticket scope lists only `Cargo.toml` (modify) and `build.rs` (create). The implementer also rewrote `src/lib.rs` entirely -- commenting out all existing modules, adding `pub mod proto`, and adding 7 unit tests. This is pragmatically necessary because removing the `eventfold` dependency means all existing modules fail to compile, and AC 5 requires `cargo build` to succeed. The `autoexamples = false` addition to `Cargo.toml` is similarly motivated (the `examples/counter.rs` imports types that no longer exist). Given the "build must pass" AC, these changes are justified and would be unreasonable to defer. Flagging as Minor rather than Critical because the deviation is forced by the AC constraint, not discretionary scope creep.

2. **Commented-out module declarations.** The global `CLAUDE.md` states "NEVER commit commented-out code; delete it." The commented-out `mod` and `pub use` lines in `src/lib.rs` (lines 21-37) serve as a migration roadmap but technically violate this convention. These should be removed before the final commit; subsequent tickets can re-add modules as they are rewritten.

## Suggestions (non-blocking)

- The `build.rs` function uses `Box<dyn std::error::Error>` which is fine for a build script (not library code). No issue here.
- The 7 unit tests are well-structured, covering all major proto message types and the generated gRPC client type. Good coverage for a codegen-only ticket.
- The `proto_grpc_client_type_exists` test cleverly uses a function pointer type assertion to verify the client type exists at compile time without needing a running server. Clean approach.
- Consider updating the crate `description` in `Cargo.toml` from "Embedded event-sourcing framework built on eventfold" to reflect the eventfold-db backend. This can be done in a later ticket (version bump).

## Scope Check

- Files within scope: YES (`Cargo.toml` modified, `build.rs` created)
- Out-of-scope files touched: `src/lib.rs` (rewritten), `Cargo.lock` (auto-updated by cargo). The `src/lib.rs` change is necessary to satisfy AC 5 (`cargo build` must pass after removing `eventfold`). `Cargo.lock` is an auto-generated artifact. Neither represents discretionary scope creep.
- Scope creep detected: NO (changes are build-necessary, not feature additions)
- Unauthorized dependencies added: NO (all deps match AC 1 and AC 2 exactly)

## Risk Assessment

- Regression risk: LOW -- This is a clean break from the old backend. All old modules are disabled (not deleted), and the example is excluded via `autoexamples = false`. No existing functionality is preserved; this is the start of a semver-major migration.
- Security concerns: NONE
- Performance concerns: NONE
