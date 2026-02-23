# PRD 001: AggregateStore::inject_event()

**Status:** DRAFT
**Created:** 2026-02-22
**Author:** PRD Writer Agent

---

## Problem Statement

When a client syncs events from a relay server, the received events are already
validated facts -- they passed `Aggregate::handle()` on the originating client and
were durably persisted there. Replaying these events through `Aggregate::handle()`
again on the receiving client is incorrect: local business rules may differ or the
reconstructed command may not even be representable. There is currently no way to write
a pre-validated `eventfold::Event` directly into a stream's JSONL file while also
notifying registered projections, which forces relay-sync implementors to bypass the
`AggregateStore` entirely and write raw JSONL themselves, losing projection integration
and deduplication guarantees.

## Goals

- Provide a single public method `AggregateStore::inject_event` that appends a
  pre-validated `eventfold::Event` directly to a stream without command validation.
- Apply the injected event to all registered projections immediately (same as events
  written through `execute`).
- Support optional process manager triggering so callers can choose whether injected
  events start sagas.
- Deduplicate by `event.id` so relay-sync scenarios can safely retry and remain
  idempotent.

## Non-Goals

- This PRD does not add relay networking, peer discovery, or any transport layer.
- It does not change the existing `execute` path -- no existing behavior is modified.
- It does not implement cross-process deduplication (the dedup index is in-memory per
  `AggregateStore` instance).
- It does not expose a bulk-inject API; batch callers loop over individual calls.
- It does not apply the injected event to the aggregate's own actor state cache; the
  actor will pick up the appended event on its next `view.refresh()` call, preserving
  existing actor isolation.
- It does not produce domain events from `Aggregate::handle()` -- the raw
  `eventfold::Event` is written as-is.

## User Stories

- As a sync engine author, I want to write relay events directly to JSONL streams so
  that I can populate a local replica without re-executing commands that may not be
  locally valid.
- As a sync engine author, I want deduplication by event ID so that I can safely retry
  inject calls without creating duplicate events in the log.
- As an application developer using projections, I want injected events to be reflected
  in projections so that read models stay consistent regardless of whether an event
  arrived via a local command or a relay sync.
- As an application developer using process managers, I want control over whether
  injected events trigger process managers so that sagas are not spuriously re-executed
  during an initial bulk import.

## Technical Approach

### New method signature (added to `src/store.rs`)

```rust
/// Options controlling the behaviour of [`AggregateStore::inject_event`].
pub struct InjectOptions {
    /// When `true`, call `run_process_managers()` after appending.
    /// Defaults to `false`.
    pub run_process_managers: bool,
}

impl Default for InjectOptions {
    fn default() -> Self {
        Self { run_process_managers: false }
    }
}

impl AggregateStore {
    pub async fn inject_event<A: Aggregate>(
        &self,
        instance_id: &str,
        event: eventfold::Event,
        opts: InjectOptions,
    ) -> io::Result<()>;
}
```

### Implementation steps (all within `src/store.rs`)

1. **Deduplication check.** If `event.id` is `Some(id)`, consult a
   `Arc<std::sync::Mutex<HashSet<String>>>` field added to `AggregateStore` (and
   propagated through its `Clone` impl, which shares `Arc`s). If the ID is already
   present, return `Ok(())` immediately without writing.

2. **Ensure stream directory exists.** Call
   `self.layout.ensure_stream(A::AGGREGATE_TYPE, instance_id)` via
   `tokio::task::spawn_blocking` (matching the existing pattern in `AggregateStore::get`).

3. **Append to JSONL.** Open an `eventfold::EventWriter` for the stream directory and
   call `writer.append(&event)` inside `spawn_blocking`. The actor for this stream (if
   alive) will pick up the new event on its next `view.refresh()` -- no direct actor
   interaction is required.

4. **Register dedup ID.** If `event.id` is `Some(id)`, insert into the seen-IDs set
   after a successful append.

5. **Apply to projections.** Acquire the `projections` `std::sync::RwLock` read lock.
   For each registered `ProjectionRunner`, call `runner.catch_up()` inside
   `spawn_blocking` (matching the existing pattern in `AggregateStore::projection`).

6. **Optionally trigger process managers.** If `opts.run_process_managers`, call
   `self.run_process_managers().await`.

### Files changed

| File | Change |
|------|--------|
| `src/store.rs` | Add `InjectOptions` struct; add `seen_ids: Arc<std::sync::Mutex<HashSet<String>>>` field to `AggregateStore`; update `Clone` impl (already manual -- not needed, all `Arc` fields clone automatically); update `AggregateStore::open` and `AggregateStoreBuilder::open` to initialize `seen_ids`; add `inject_event` method. |
| `src/lib.rs` | Re-export `InjectOptions`. |

### Interaction with the actor

The actor exclusively owns its `EventWriter`, so `inject_event` must open a _separate_
`EventWriter` for the append (or close the actor's writer first). Because
`eventfold::EventWriter` uses `flock` for exclusive access, injecting while an actor is
alive for the same stream will block until the actor releases the lock (on idle eviction
or shutdown).

To avoid blocking callers indefinitely, the preferred approach is to open a _second_
`EventWriter` via `spawn_blocking`. If `eventfold::EventWriter::open` uses a shared
(advisory) lock that allows multiple writers, appends are safe. If it uses an exclusive
lock, inject must either (a) send an `InjectEvent` message to the actor (keeping the
actor as the single writer) or (b) document that concurrent actor + inject on the same
stream may block.

**Resolution for this PRD:** add a new `ActorMessage::Inject` variant so the actor
remains the sole writer for any stream that has a live actor. For streams with no live
actor, open a temporary writer directly. This is the safest path and avoids lock
contention entirely.

Concretely:

- If `self.cache` contains a live `AggregateHandle<A>` for `(A::AGGREGATE_TYPE, instance_id)`,
  send an `ActorMessage::Inject { event, reply }` to the actor and await the reply.
- Otherwise, open a temporary `EventWriter` in `spawn_blocking` and append directly.

This requires adding `ActorMessage::Inject` and handling it in `run_actor` in
`src/actor.rs`.

### Dedup index scope

The `seen_ids` `HashSet` is unbounded and in-memory only. It is reset when the
`AggregateStore` is dropped and rebuilt. This is intentional for Phase 1 of this
feature. A persistent dedup index (e.g. written to `meta/seen_ids.jsonl`) is a
potential future enhancement outside this PRD's scope.

## Acceptance Criteria

1. Calling `store.inject_event::<A>("id", event, InjectOptions::default()).await`
   returns `Ok(())` and the injected event appears as the last line of
   `<base>/streams/<agg_type>/<id>/app.jsonl`.
2. After `inject_event` returns, calling `store.projection::<P>()` on a registered
   projection that subscribes to `A::AGGREGATE_TYPE` reflects the injected event (i.e.
   the projection's count or state changes as if the event had arrived via `execute`).
3. When `event.id` is `Some("ev-1")` and `inject_event` is called twice with the same
   event, `app.jsonl` contains exactly one copy of that event after both calls.
4. When `event.id` is `None`, two calls with structurally identical events both write
   to `app.jsonl` (no deduplication for ID-less events).
5. When `InjectOptions { run_process_managers: false }` (the default), calling
   `inject_event` followed by `run_process_managers` explicitly dispatches one envelope
   per injected event (verifying PMs were NOT auto-triggered mid-inject).
6. When `InjectOptions { run_process_managers: true }`, calling `inject_event` alone
   dispatches the expected process-manager commands without a separate
   `run_process_managers` call.
7. Injecting an event into a stream that has a live actor does not return
   `Err(io::ErrorKind::WouldBlock)` or deadlock; the call completes within 1 second
   under normal conditions.
8. After `inject_event` is called for a stream with no existing directory, a subsequent
   `store.get::<A>("id").await` and `handle.state()` reflects the injected event in the
   aggregate state (actor replays from JSONL on spawn).
9. `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` all pass with
   no new failures after the implementation.
10. `InjectOptions` and `AggregateStore::inject_event` are re-exported from `src/lib.rs`
    and appear in `cargo doc --no-deps` output with complete doc comments.

## Open Questions

- Does `eventfold::EventWriter::open` use a shared or exclusive flock? If shared, the
  simpler "open a temporary writer" approach (without `ActorMessage::Inject`) works.
  The technical approach above assumes exclusive locking and documents the actor-message
  path accordingly. The implementor should verify this from the `eventfold` 0.2.0
  source before choosing the approach.
- Should the dedup `seen_ids` index be persisted to disk (e.g.
  `meta/seen_ids.jsonl`) to survive process restarts? This PRD scopes dedup to
  in-memory only. If persistence is needed, a follow-on PRD should be filed.
- Should `inject_event` be generic over `A: Aggregate` (as specified here) or take
  `aggregate_type: &str` + `instance_id: &str` as raw strings? The generic form is
  type-safe but requires the caller to have the aggregate type in scope; the raw-string
  form is more convenient for generic sync engines. This PRD uses the generic form;
  a raw-string overload can be added as a follow-on.

## Dependencies

- `eventfold` 0.2.0 -- `EventWriter::append`, `EventReader`, `Event` (including the
  `id: Option<String>` field confirmed in project memory).
- Existing `AggregateStore`, `StreamLayout`, `ProjectionRunner`, and `ProcessManagerCatchUp`
  infrastructure in `src/store.rs` and `src/projection.rs`.
- `ActorMessage` enum in `src/actor.rs` -- requires a new `Inject` variant if the
  exclusive-lock path is taken (see Open Questions).
