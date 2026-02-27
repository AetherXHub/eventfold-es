# Code Review: Ticket 5 -- src/snapshot.rs: local file-based snapshot module

**Ticket:** 5 -- src/snapshot.rs: local file-based snapshot module
**Impl Report:** docs/prd/009-eventfold-db-backend-reports/ticket-05-impl.md
**Date:** 2026-02-27 15:30
**Verdict:** APPROVED

---

## AC Coverage

| AC # | Description | Status | Notes |
|------|-------------|--------|-------|
| 1 | `Snapshot<A>` struct with `state: A` and `stream_version: u64`; derives `Serialize, Deserialize` where `A: Serialize + DeserializeOwned` | Met | Lines 24-31 of `src/snapshot.rs`. Uses `#[serde(bound(serialize = "A: Serialize", deserialize = "A: DeserializeOwned"))]` which correctly handles the `DeserializeOwned` vs `Deserialize<'de>` bound conflict. Fields are `pub state: A` and `pub stream_version: u64`. |
| 2 | `snapshot_path` returns `<base_dir>/snapshots/<aggregate_type>/<instance_id>/snapshot.json` | Met | Lines 44-50. Joins segments via `Path::join`. Test `snapshot_path_returns_expected_path` (line 133) verifies exact path `/data/myapp/snapshots/counter/c-1/snapshot.json`. |
| 3 | `save_snapshot` writes atomically via temp-rename | Met | Lines 67-84. Writes to `snapshot.json.tmp` via `with_extension("json.tmp")`, then `fs::rename` to final path. Same-directory rename is atomic on POSIX. Creates parent dirs via `create_dir_all`. |
| 4 | `load_snapshot` returns `Ok(None)` for missing files; logs warning and returns `Ok(None)` on deserialization failure | Met | Lines 102-124. `NotFound` error kind returns `Ok(None)` (line 109). Other I/O errors propagate as `Err` (line 110). Deserialization failure logs `tracing::warn!` (line 116) and returns `Ok(None)` (line 121). |
| 5 | Test: save then load roundtrip | Met | `save_then_load_roundtrips` (lines 143-157). Saves `Counter { value: 42 }` at `stream_version: 7`, loads back, asserts both fields match. |
| 6 | Test: load nonexistent returns `Ok(None)` | Met | `load_nonexistent_returns_none` (lines 159-165). Loads from fresh tempdir with "no-such-id", asserts `Ok(None)`. |
| 7 | Test: corrupt JSON returns `Ok(None)` | Met | `load_corrupt_json_returns_none` (lines 167-179). Writes `"this is not valid json!!!"` to the snapshot path, loads, asserts `Ok(None)` not `Err`. |
| 8 | Test: atomic temp rename (temp file absent after save) | Met | `save_uses_atomic_temp_rename` (lines 183-200). After save, asserts final file exists and `.tmp` file does not. |
| 9 | Test: `snapshot_path` returns expected path | Met | `snapshot_path_returns_expected_path` (lines 132-139). Verifies exact `PathBuf`. |
| 10 | Quality gates pass | Met | Verified independently: `cargo build` (zero warnings), `cargo clippy -- -D warnings` (clean), `cargo fmt --check` (clean), `cargo test` (62 tests pass including 5 snapshot tests). |

## Issues Found

### Critical (must fix before merge)

None.

### Major (should fix, risk of downstream problems)

None.

### Minor (nice to fix, not blocking)

1. **`#![allow(dead_code)]` as inner attribute** (`src/snapshot.rs:3`): The inner attribute `#![allow(dead_code)]` is placed before the module doc comment at line 5. While syntactically valid and functional, the conventional order in Rust is module doc comments (`//!`) first, then inner attributes. The comment at lines 1-2 explains the rationale clearly. This will be naturally removed when Ticket 8 (actor rewrite) consumes the snapshot functions. No action needed.

2. **Impl report inaccuracy re: client.rs**: The impl report claims "had to comment out `mod client;` and `pub use client::EsClient;`" but the current `src/lib.rs` has both active (lines 34-35). This was likely resolved by the Ticket 4 implementer working in parallel. The final state is correct; the report text is stale. No code action needed.

## Suggestions (non-blocking)

- The `Snapshot<A>` struct does not derive `PartialEq`. If `Aggregate` ever adds a `PartialEq` bound (or test readability is desired), this could be added with a serde-like conditional bound: `#[derive(PartialEq)]` with `A: PartialEq`. Not needed now since tests compare individual fields.

- `save_snapshot` uses `serde_json::to_vec_pretty` (line 79) which includes whitespace formatting. For snapshot files that are not human-inspected, `serde_json::to_vec` would produce smaller files. The difference is negligible for aggregate state sizes typical of CRM-scale applications. Not a concern.

## Scope Check

- Files within scope: YES
  - `src/snapshot.rs` (created) -- in scope
  - `src/lib.rs` (modified: added `mod snapshot;` at line 36) -- not explicitly listed in ticket scope ("Create: `src/snapshot.rs`" only) but is the minimal necessary change to register the module. Other tickets (e.g., Ticket 2, 4) list lib.rs explicitly; Ticket 5 does not. The single-line addition is a reasonable implicit requirement, not scope creep.
- Scope creep detected: NO
- Unauthorized dependencies added: NO (`tempfile` was already in dev-dependencies from Ticket 1)

## Risk Assessment

- Regression risk: LOW -- New module with no consumers yet. All 62 existing tests pass. No changes to existing production code paths.
- Security concerns: NONE -- File I/O is local disk only; no network, no secrets.
- Performance concerns: NONE -- Single JSON file read/write per snapshot operation; `serde_json::to_vec_pretty` is marginally slower than `to_vec` but irrelevant at this scale.
