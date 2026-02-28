# PRD 011: Extract eventfold-proto Shared Crate

**Status:** DRAFT
**Created:** 2026-02-28
**Author:** PRD Writer Agent

---

## Problem Statement

`eventfold.proto` is independently vendored in both `eventfold-es` and
`eventfold-db`. Every time the gRPC API evolves, both repos must be updated in
lock-step and `eventfold-es` CI performs a full clone of `eventfold-db` on
every push just to assert the two copies are identical. This clone adds network
latency to CI, couples the two repos at the workflow level rather than the
dependency level, and will silently allow divergence if either copy is edited
without updating the other.

## Goals

- Create a standalone `eventfold-proto` crate that is the single authoritative
  owner of `eventfold.proto` and its generated Rust types.
- Remove the vendored `proto/eventfold.proto` file and all proto-compilation
  infrastructure from `eventfold-es`.
- Remove the "Check vendored proto is in sync" CI step from
  `.github/workflows/ci.yml` in `eventfold-es`.
- Preserve full backwards source compatibility in `eventfold-es` by
  re-exporting `eventfold_proto` as `pub mod proto` so no downstream callsite
  changes.

## Non-Goals

- Changing any gRPC API surface or message definitions in `eventfold.proto`.
- Modifying `eventfold-db` — that work is a separate issue to be filed against
  that repo.
- Publishing `eventfold-proto` to crates.io; a git dependency is sufficient.
- Introducing a Cargo workspace; `eventfold-proto` lives in its own repo.
- Changing any public API of `eventfold-es` beyond swapping the proto module
  source.

## User Stories

- As a maintainer updating the gRPC API, I want one place to edit the `.proto`
  file, so that both `eventfold-es` and `eventfold-db` consume the change via
  a dependency bump rather than two manual copy-paste edits.
- As a contributor to `eventfold-es`, I want CI to not clone an unrelated repo
  on every push, so that builds are faster and do not fail due to transient
  network issues fetching `eventfold-db`.
- As a consumer of `eventfold-es`, I want `crate::proto::*` to keep working
  after this change, so that my application code requires no modifications.

## Technical Approach

### New repo: `eventfold-proto`

Create a new repository at `git@github.com:AetherXHub/eventfold-proto.git`
(GitHub org confirmed as `AetherXHub` from `Cargo.toml` repository field and
CI clone URL).

**File layout:**

```
eventfold-proto/
  Cargo.toml
  build.rs
  src/
    lib.rs
  proto/
    eventfold.proto
```

**`Cargo.toml`** — minimal; no runtime dependencies beyond `tonic` (for
`include_proto!`) and `prost` (re-exported types):

```toml
[package]
name = "eventfold-proto"
version = "0.1.0"
edition = "2024"
description = "Generated protobuf types for the eventfold gRPC service"
license = "MIT OR Apache-2.0"
repository = "https://github.com/aetherxhub/eventfold-proto"

[dependencies]
prost = "0.13"
tonic = "0.13"

[build-dependencies]
tonic-build = "0.13"
```

**`build.rs`** — identical to the current `eventfold-es` build script:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_path = "proto/eventfold.proto";
    println!("cargo:rerun-if-changed={proto_path}");
    tonic_build::compile_protos(proto_path)?;
    Ok(())
}
```

**`src/lib.rs`** — re-exports all generated types at crate root:

```rust
//! Generated protobuf/gRPC types for the `eventfold` service.
tonic::include_proto!("eventfold");
```

The `.proto` file is copied verbatim from
`eventfold-es/proto/eventfold.proto` as the starting content.

### Changes to `eventfold-es`

| File | Change |
|------|--------|
| `proto/eventfold.proto` | Delete |
| `build.rs` | Delete (entire file) |
| `Cargo.toml` | Remove `tonic-build` from `[build-dependencies]`; add `eventfold-proto` git dependency under `[dependencies]` |
| `src/lib.rs` | Replace inline `tonic::include_proto!` block with `pub use eventfold_proto as proto;` |
| `.github/workflows/ci.yml` | Remove the "Install protoc" step and the "Check vendored proto is in sync" step |

**`Cargo.toml` dependency addition:**

```toml
[dependencies]
# ... existing deps ...
eventfold-proto = { git = "https://github.com/AetherXHub/eventfold-proto.git", tag = "v0.1.0" }
```

The `tonic-build` entry in `[build-dependencies]` is removed entirely.
`prost = "0.13"` in `[dependencies]` is retained because `eventfold-es` uses
`prost::Message` directly in `src/event.rs`.

**`src/lib.rs` proto block replacement:**

```rust
// Before
pub mod proto {
    tonic::include_proto!("eventfold");
}

// After — re-export the upstream crate under the existing `proto` name so
// all `crate::proto::*` call sites throughout src/ continue to compile
// without modification.
pub use eventfold_proto as proto;
```

All `crate::proto::*` references across `src/actor.rs`, `src/client.rs`,
`src/event.rs`, `src/live.rs`, `src/process_manager.rs`, `src/projection.rs`,
and `src/store.rs` resolve through this re-export unchanged.

**CI diff** — remove both steps from `.github/workflows/ci.yml`:

```yaml
# Remove:
- name: Install protoc
  run: |
    curl -fsSL "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-linux-x86_64.zip" -o protoc.zip
    unzip -q protoc.zip -d /usr/local
    protoc --version
- name: Check vendored proto is in sync
  run: |
    git clone --depth 1 https://github.com/AetherXHub/eventfold-db.git /tmp/eventfold-db
    diff proto/eventfold.proto /tmp/eventfold-db/proto/eventfold.proto
```

The `PROTOC_VERSION` env var in the CI file is also removed as it becomes
unused. Note: `tonic-build` shells out to `protoc` at build time, so
`eventfold-proto`'s own CI will need a protoc installation step; however,
`eventfold-es` CI no longer needs it because `eventfold-proto` is depended on
as a pre-built git source and Cargo builds it (which requires protoc only when
`eventfold-proto` itself is first compiled — Cargo's build caching means this
is a one-time cost per runner cache miss).

### Dependency version pinning

Use a `tag` in the git dependency (`tag = "v0.1.0"`) rather than a bare `rev`
or branch reference so that `Cargo.lock` resolves deterministically and
`--locked` builds remain stable in CI.

## Acceptance Criteria

1. A repository `AetherXHub/eventfold-proto` exists containing exactly:
   `Cargo.toml`, `build.rs`, `src/lib.rs`, and `proto/eventfold.proto` (the
   last file byte-for-byte identical to the current
   `eventfold-es/proto/eventfold.proto`).
2. `cargo build --locked` in `eventfold-proto` succeeds with zero warnings.
3. `eventfold-es/proto/` directory does not exist in the repository after this
   change.
4. `eventfold-es/build.rs` does not exist in the repository after this change.
5. `eventfold-es/Cargo.toml` contains no `[build-dependencies]` section (or
   one with no entries), and lists `eventfold-proto` as a git dependency
   pinned to a tag.
6. `cargo build --locked` in `eventfold-es` succeeds with zero warnings after
   the change.
7. `cargo test --locked` in `eventfold-es` passes with all tests green,
   including the six proto accessibility tests in `src/lib.rs`.
8. `cargo clippy --all-targets --all-features --locked -- -D warnings` in
   `eventfold-es` exits with code 0.
9. `.github/workflows/ci.yml` in `eventfold-es` contains no step whose `name`
   field is "Install protoc" or "Check vendored proto is in sync", and
   contains no reference to the string `eventfold-db`.
10. All `crate::proto::*` references across `src/actor.rs`, `src/client.rs`,
    `src/event.rs`, `src/live.rs`, `src/process_manager.rs`,
    `src/projection.rs`, and `src/store.rs` compile without modification
    (verified by `cargo build --locked` passing in AC 6).

## Open Questions

- Does `eventfold-db` need to be updated to depend on `eventfold-proto` as
  well, or will it continue to vendor its own copy of the proto file until a
  separate issue is filed? The scope of this PRD excludes modifying
  `eventfold-db`, but the eventfold-proto README should note that
  `eventfold-db` is a known consumer that has not yet migrated.
- Should `eventfold-proto` CI also run a `protoc` lint step (e.g.
  `buf lint`) to catch proto style violations at the source? Default decision:
  no; keep the new crate minimal. Add buf linting in a follow-on PRD if
  desired.

## Dependencies

- `AetherXHub/eventfold-proto` repo must be created and tagged `v0.1.0` before
  the `eventfold-es` dependency update can be merged.
- `tonic` 0.13 and `prost` 0.13 must remain compatible between `eventfold-es`
  and `eventfold-proto`; both crates pin the same versions so no conflict
  arises.
- GitHub Actions runner must have internet access to clone the
  `eventfold-proto` git dependency during `cargo build` (currently satisfied
  since the runner already clones `eventfold-db` in the drift check step being
  removed).
