# PRD 005: AggregateStore Stream Enumeration API

**Status:** TICKETS READY
**Created:** 2026-02-22
**Author:** PRD Writer Agent
**GitHub Issue:** #5

---

## Problem Statement

During the push phase of sync, a client must walk every local aggregate stream to find
events written after a given timestamp. `AggregateStore` currently exposes only a
type-scoped `list<A: Aggregate>()` method that requires a compile-time aggregate type
parameter. There is no way for type-agnostic infrastructure code (e.g. a sync engine)
to enumerate all streams regardless of aggregate type, and no public method for reading
raw `eventfold::Event` records from a named stream without spawning an actor.

## Goals

- Enable type-agnostic enumeration of all aggregate streams stored in an
  `AggregateStore`, optionally filtered by aggregate type string.
- Enable type-agnostic reading of all raw `eventfold::Event` records from any stream
  identified by `(aggregate_type, instance_id)` pair.
- Keep the change strictly additive: no existing public API signatures, behavior, or
  on-disk format is altered.

## Non-Goals

- Filtering events by timestamp or offset inside the new methods. Callers perform their
  own filtering on the returned `Vec<eventfold::Event>`.
- Pagination or streaming/iterator-based results. Both methods return an owned `Vec`.
- Writing or mutating events through these new methods.
- Cross-process synchronization or distributed sync protocol. These methods expose local
  file state only.
- Modifications to `StreamLayout`, `AggregateHandle`, or any module other than
  `src/store.rs` and `src/lib.rs`.

## User Stories

- As a sync engine developer, I want to call `store.list_streams(None)` and receive
  every `(aggregate_type, instance_id)` pair on disk, so that I can identify which
  streams exist without hard-coding aggregate type names.
- As a sync engine developer, I want to call `store.list_streams(Some("order"))` and
  receive only the `(aggregate_type, instance_id)` pairs for the `order` aggregate type,
  so that I can scope a sync pass to a single domain area.
- As a sync engine developer, I want to call `store.read_events("order", "ord-001")` and
  receive all raw `eventfold::Event` records from that stream, so that I can inspect
  timestamps and payloads to determine what to push.

## Technical Approach

Both new methods are added to `impl AggregateStore` in `src/store.rs` and re-exported
from `src/lib.rs` only as methods on the existing public type (no new public types are
introduced).

### `list_streams`

```rust
pub async fn list_streams(
    &self,
    aggregate_type: Option<&str>,
) -> io::Result<Vec<(String, String)>>
```

**Implementation:**

- Delegates to `tokio::task::spawn_blocking` (matching the pattern of the existing
  `list<A>` method) to avoid blocking the async executor on directory I/O.
- When `aggregate_type` is `Some(agg_type)`:
  - Calls `self.layout.list_streams(agg_type)` to get instance IDs.
  - Maps each instance ID to `(agg_type.to_owned(), instance_id)`.
- When `aggregate_type` is `None`:
  - Reads `<base_dir>/streams/` with `fs::read_dir`, collecting each subdirectory name
    as an aggregate type.
  - For each aggregate type directory, calls `self.layout.list_streams(agg_type)`.
  - Flattens the results into a single `Vec<(String, String)>`.
  - Sorts the result: primary key = aggregate type (lexicographic), secondary key =
    instance ID (lexicographic). This produces a deterministic, stable ordering.
- Returns an empty `Vec` (not an error) if no streams exist.

The `None` path requires a small new helper on `StreamLayout`:
`pub(crate) fn list_aggregate_types(&self) -> io::Result<Vec<String>>`, which reads
`<base_dir>/streams/` and returns a sorted `Vec<String>` of subdirectory names. This
helper is `pub(crate)` and not part of the public API surface.

### `read_events`

```rust
pub async fn read_events(
    &self,
    aggregate_type: &str,
    instance_id: &str,
) -> io::Result<Vec<eventfold::Event>>
```

**Implementation:**

- Constructs the stream directory path via `self.layout.stream_dir(aggregate_type, instance_id)`.
- Delegates to `tokio::task::spawn_blocking`.
- Inside the blocking closure, creates a fresh `eventfold::EventReader::new(&stream_dir)`
  (matching the pattern already used in `ProjectionRunner::catch_up` in
  `src/projection.rs`).
- Calls `reader.read_from(0)` to iterate all events from the beginning.
- Collects the `eventfold::Event` values into a `Vec`, propagating any I/O error.
- If the stream directory exists but `app.jsonl` does not yet exist (new stream with no
  events written), `read_from` returns `ErrorKind::NotFound`; this case is mapped to an
  empty `Vec` (not an error), consistent with the pattern in `ProjectionRunner::catch_up`.
- If the stream directory itself does not exist, returns `io::Error` with
  `ErrorKind::NotFound` -- this signals a programming error (the caller asked for a
  stream that was never created).

**Files changed:**

| File | Change |
|------|--------|
| `src/storage.rs` | Add `pub(crate) fn list_aggregate_types(&self) -> io::Result<Vec<String>>` to `StreamLayout` |
| `src/store.rs` | Add `pub async fn list_streams(...)` and `pub async fn read_events(...)` to `impl AggregateStore` |
| `src/lib.rs` | No change required; `AggregateStore` is already fully re-exported and `eventfold::Event` is already a transitive public dependency |

**Note on `eventfold::Event` in the public API:** `eventfold` is a direct dependency
declared in `Cargo.toml` and `eventfold::Event` is already used in the `ProcessManager`
trait's `react` method signature (`pub` trait, `pub` method). Adding it to
`read_events`'s return type does not introduce a new transitive public dependency.

## Acceptance Criteria

1. `AggregateStore::list_streams(None)` called on a store with streams for two
   aggregate types (`"counter"` with instances `["c-1", "c-2"]` and `"toggle"` with
   instances `["t-1"]`) returns exactly
   `[("counter", "c-1"), ("counter", "c-2"), ("toggle", "t-1")]` in that order
   (primary sort by aggregate type, secondary sort by instance ID).

2. `AggregateStore::list_streams(Some("counter"))` called on the same store as AC 1
   returns exactly `[("counter", "c-1"), ("counter", "c-2")]`.

3. `AggregateStore::list_streams(None)` called on a freshly opened store (no streams
   yet created) returns `Ok(vec![])`.

4. `AggregateStore::list_streams(Some("nonexistent"))` called on any store where that
   aggregate type has no streams returns `Ok(vec![])`.

5. `AggregateStore::read_events("counter", "c-1")` called after two `Increment`
   commands have been executed on instance `"c-1"` returns a `Vec<eventfold::Event>` of
   length 2, where `event[0].event_type == "Incremented"` and
   `event[1].event_type == "Incremented"`.

6. `AggregateStore::read_events("counter", "c-1")` called on a stream directory that
   exists (via `store.get::<Counter>("c-1").await`) but has had no commands executed
   returns `Ok(vec![])` (empty, not an error).

7. `AggregateStore::read_events("nonexistent", "x")` called where that stream directory
   has never been created returns `Err(e)` where `e.kind() == io::ErrorKind::NotFound`.

8. `cargo test` passes with no new test failures and no compiler warnings.

9. `cargo clippy -- -D warnings` produces no new diagnostics.

10. Both new methods have doc comments following the project's established pattern:
    summary line, `# Arguments`, `# Returns`, `# Errors` sections, as seen in existing
    `AggregateStore` methods such as `get` and `list`.

## Open Questions

- **`list_streams(None)` and the `meta/streams.jsonl` registry:** the registry file
  already tracks every stream that has ever been `ensure_stream`'d, which would provide
  an alternative to a filesystem scan for the `None` case. However, deleted streams
  (if any) would remain in the registry. The filesystem-scan approach (reading
  `<base_dir>/streams/`) is more authoritative about what currently exists and is
  consistent with how `list_streams(&str)` already works. This PRD specifies the
  filesystem-scan approach unless the maintainer prefers registry-based enumeration.

- **Should `read_events` accept an `offset` parameter?** The issue description says the
  sync client needs to find events newer than a last sync timestamp; it doesn't need a
  byte-offset API because timestamps are in the event payload. This PRD defaults to
  always reading from offset 0. If an offset parameter is needed later it can be added
  without breaking the zero-arg form.

## Dependencies

- `eventfold` 0.2.0 — `EventReader::new`, `EventReader::read_from`, `eventfold::Event`
  (already a declared dependency; no version change needed).
- `tokio` — `tokio::task::spawn_blocking` (already used throughout `src/store.rs`).
- Existing `StreamLayout` methods: `stream_dir`, `list_streams` (already implemented in
  `src/storage.rs`).
