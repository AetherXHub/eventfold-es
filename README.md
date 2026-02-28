# eventfold-es

Event-sourcing framework backed by [eventfold-db](https://github.com/AetherXHub/eventfold-db), a single-node gRPC event store with global ordering, optimistic concurrency, and push-based subscriptions.

Designed for multi-process, multi-machine applications that need consistent event sourcing without the operational overhead of a distributed database.

## Features

- **Aggregates** -- define domain models with command handlers and event applicators
- **Actor-per-instance** -- each aggregate runs as a tokio task with optimistic concurrency (3x retry)
- **Projections** -- cross-stream read models via `SubscribeAll` with global cursor checkpointing
- **Process managers** -- cross-aggregate workflows that react to events with commands
- **Live subscriptions** -- persistent `SubscribeAll` stream that keeps projections always-current and process managers continuously reactive, with configurable checkpointing and exponential backoff reconnection
- **Snapshots** -- local file-based snapshots for fast actor recovery without full stream replay
- **Idle eviction** -- actors shut down after configurable inactivity, re-spawn transparently
- **Tracing** -- structured instrumentation via the `tracing` crate throughout

## Prerequisites

A running [eventfold-db](https://github.com/AetherXHub/eventfold-db) server. The default endpoint is `http://127.0.0.1:2113`.

## Quick Start

```rust
use eventfold_es::{Aggregate, AggregateStoreBuilder, CommandContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Counter { value: u64 }

#[derive(Clone)]
enum CounterCommand { Increment }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum CounterEvent { Incremented }

#[derive(Debug, thiserror::Error)]
enum CounterError {}

impl Aggregate for Counter {
    const AGGREGATE_TYPE: &'static str = "counter";
    type Command = CounterCommand;
    type DomainEvent = CounterEvent;
    type Error = CounterError;

    fn handle(&self, cmd: CounterCommand) -> Result<Vec<CounterEvent>, CounterError> {
        match cmd {
            CounterCommand::Increment => Ok(vec![CounterEvent::Incremented]),
        }
    }

    fn apply(mut self, _event: &CounterEvent) -> Self {
        self.value += 1;
        self
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = AggregateStoreBuilder::new()
        .endpoint("http://127.0.0.1:2113")
        .base_dir("/tmp/my-app")
        .open()
        .await?;

    let handle = store.get::<Counter>("counter-1").await?;
    handle.execute(CounterCommand::Increment, CommandContext::default()).await?;
    let state = handle.state().await?;
    assert_eq!(state.value, 1);

    Ok(())
}
```

See [`examples/counter.rs`](examples/counter.rs) for a full example with projections, multiple instances, and resets.

## Core Types

| Type | Role |
|------|------|
| [`Aggregate`] | Domain model: handles commands, emits events, folds state |
| [`AggregateStoreBuilder`] | Fluent builder for configuring the store with endpoint, projections, and PMs |
| [`AggregateStore`] | Central registry: spawns actors, caches handles, runs projections |
| [`Projection`] | Cross-stream read model built from `StoredEvent`s via `SubscribeAll` |
| [`ProcessManager`] | Cross-aggregate workflow that reacts to events with commands |
| [`AggregateHandle`] | Async handle to a running aggregate actor |
| [`StoredEvent`] | Decoded event delivered to projections/PMs with aggregate type, instance ID, and timestamp |
| [`CommandContext`] | Cross-cutting metadata (actor identity, correlation ID, source device) |
| [`LiveConfig`] | Tuning knobs for checkpoint interval and reconnect backoff in live mode |
| [`LiveHandle`] | Control handle for the live subscription loop (shutdown, caught-up check) |

[`Aggregate`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.Aggregate.html
[`AggregateStore`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateStore.html
[`AggregateStoreBuilder`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateStoreBuilder.html
[`Projection`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.Projection.html
[`ProcessManager`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.ProcessManager.html
[`AggregateHandle`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateHandle.html
[`StoredEvent`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.StoredEvent.html
[`CommandContext`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.CommandContext.html
[`LiveConfig`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.LiveConfig.html
[`LiveHandle`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.LiveHandle.html

## Architecture

```
                    eventfold-es                          eventfold-db
                    ============                          ============
AggregateStoreBuilder                                   gRPC server
  .endpoint("http://...")                               (global log + streams)
  .open().await
       |
AggregateStore
  handle cache (Arc<RwLock>)
       |
  +----+----+
  |         |
Actor       ProjectionRunner / ProcessManagerRunner
(tokio task)   subscribe_all_from(global_position)
  |                |
  |           Live subscription loop (optional)
  |             holds stream open past CaughtUp
  |             fans out events to all projections/PMs
  |                |
  EsClient         EsClient
  .append()        .subscribe_all_from()
  .read_stream()
       |                |
       +--- gRPC -------+---> eventfold-db :2113
```

Each aggregate instance runs as a **tokio task** (not a blocking thread). The actor owns the in-memory state and processes commands sequentially. Writes use `ExpectedVersion::Exact(v)` for optimistic concurrency with up to 3 automatic retries on conflict.

Projections and process managers consume the **global event log** via `SubscribeAll`, replacing per-stream cursors with a single `global_position` checkpoint.

Idle actors shut down after a configurable timeout (default 5 minutes), saving a local snapshot. The next `store.get()` transparently re-spawns the actor and recovers from the snapshot + any new events.

## Local Storage Layout

Local caches live under `base_dir` (snapshots and checkpoints only -- events are in eventfold-db):

```
<base_dir>/
    snapshots/<aggregate_type>/<instance_id>/
        snapshot.json               # aggregate state + stream version
    projections/<name>/
        checkpoint.json             # projection state + global position
    process_managers/<name>/
        checkpoint.json             # PM state + global position
        dead_letters.jsonl          # failed command dispatches
```

## Configuration

```rust
let store = AggregateStoreBuilder::new()
    .endpoint("http://127.0.0.1:2113")
    .base_dir("/tmp/my-app")
    .projection::<MyProjection>()
    .process_manager::<MySaga>()
    .aggregate_type::<TargetAggregate>()  // dispatch target for process managers
    .idle_timeout(Duration::from_secs(600))
    .open()
    .await?;
```

- **`endpoint(url)`** -- eventfold-db gRPC server address
- **`base_dir(path)`** -- local directory for snapshots and checkpoints
- **`projection::<P>()`** -- register a read model
- **`process_manager::<PM>()`** -- register a workflow coordinator
- **`aggregate_type::<A>()`** -- register a dispatch target (requires `A::Command: DeserializeOwned`)
- **`idle_timeout(dur)`** -- set actor eviction timeout (default: 5 min)
- **`live_config(config)`** -- set [`LiveConfig`] for checkpoint interval and reconnect backoff

## Live Subscriptions

By default, projections and process managers are **pull-based**: each call to `store.projection::<P>()` opens a `SubscribeAll` stream, catches up, and drops it. Live mode holds the stream open, keeping projections always-current and process managers continuously reactive.

```rust
use eventfold_es::LiveConfig;
use std::time::Duration;

// Start live mode (call once, e.g. at app startup)
let live = store.start_live().await?;

// Read projections instantly -- no catch-up latency
let summary = store.live_projection::<PipelineSummary>().await?;

// Process managers run automatically in the background.
// No need to call store.run_process_managers() after writes.

// Check if historical replay is complete
if live.is_caught_up() {
    println!("all caught up, live tail active");
}

// Graceful shutdown (saves final checkpoints)
live.shutdown().await?;
```

The pull-based `store.projection::<P>()` and `store.run_process_managers()` remain fully functional whether or not live mode is active. `live_projection::<P>()` falls back to pull-based catch-up when live mode is not started.

### LiveConfig

Configure checkpoint frequency and reconnect behavior via the builder:

```rust
let store = AggregateStoreBuilder::new()
    .endpoint("http://127.0.0.1:2113")
    .base_dir("/tmp/my-app")
    .projection::<MyProjection>()
    .process_manager::<MySaga>()
    .aggregate_type::<Target>()
    .live_config(LiveConfig {
        checkpoint_interval: Duration::from_secs(10),
        reconnect_base_delay: Duration::from_millis(500),
        ..LiveConfig::default()
    })
    .open()
    .await?;
```

| Field | Default | Description |
|-------|---------|-------------|
| `checkpoint_interval` | 5s | How often to flush checkpoints to disk during live mode |
| `reconnect_base_delay` | 1s | Initial backoff delay after a stream disconnection |
| `reconnect_max_delay` | 30s | Cap on exponential backoff between reconnect attempts |

Checkpoints are also saved on graceful shutdown and before each reconnect attempt.

## Stream ID Mapping

eventfold-db uses UUIDs for stream IDs. eventfold-es derives them deterministically:

```
stream_uuid("counter", "c-1") = UUID v5(NAMESPACE, "counter/c-1")
```

The aggregate type and instance ID are also stamped into each event's metadata, making events self-describing without a client-side registry.

## Domain Event Convention

All `DomainEvent` enums must use adjacently tagged serde serialization:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum MyEvent {
    Created { name: String },  // -> {"type": "Created", "data": {"name": "..."}}
    Deleted,                   // -> {"type": "Deleted"}
}
```

The `type` field maps to `event_type` in eventfold-db; `data` becomes the event payload.

## License

MIT
