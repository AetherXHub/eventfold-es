# eventfold-es

Embedded event-sourcing framework built on [eventfold](https://crates.io/crates/eventfold).

No external database, message broker, or network service required -- all state is persisted to plain files on disk. Designed for single-binary CLIs, local-first desktop applications, embedded devices, and prototypes.

## Features

- **Aggregates** -- define domain models with command handlers and event applicators
- **Actor-per-instance** -- each aggregate runs on a dedicated thread with exclusive file lock
- **Projections** -- cross-stream read models with incremental catch-up and checkpointing
- **Process managers** -- cross-aggregate workflows that react to events with commands
- **CommandBus** -- typed command routing by `TypeId` (no serialization needed)
- **Idle eviction** -- actors shut down after configurable inactivity, re-spawn transparently
- **Tracing** -- structured instrumentation via the `tracing` crate throughout

## Quick Start

```rust
use eventfold_es::{Aggregate, AggregateStore, CommandBus, CommandContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Counter { value: u64 }

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
    let store = AggregateStore::open("/tmp/my-app").await?;

    // Direct handle usage
    let handle = store.get::<Counter>("counter-1").await?;
    handle.execute(CounterCommand::Increment, CommandContext::default()).await?;
    let state = handle.state().await?;
    assert_eq!(state.value, 1);

    // Or use the CommandBus
    let mut bus = CommandBus::new(store);
    bus.register::<Counter>();
    bus.dispatch("counter-2", CounterCommand::Increment, CommandContext::default()).await?;

    Ok(())
}
```

See [`examples/counter.rs`](examples/counter.rs) for a full example with projections and multiple instances.

For a real-world example, see [eventfold-crm](https://github.com/AetherXHub/eventfold-crm) -- a Tauri v2 desktop CRM built on eventfold-es with aggregates, projections, process managers, and a React frontend.

## Core Types

| Type | Role |
|------|------|
| [`Aggregate`] | Domain model: handles commands, emits events, folds state |
| [`AggregateStore`] | Central registry: spawns actors, caches handles, runs projections |
| [`AggregateStoreBuilder`] | Fluent builder for configuring projections, process managers, and timeouts |
| [`Projection`] | Cross-stream read model built from events |
| [`ProcessManager`] | Cross-aggregate workflow that reacts to events with commands |
| [`CommandBus`] | Typed command router keyed by `TypeId` |
| [`AggregateHandle`] | Async handle to a running aggregate actor |
| [`CommandContext`] | Cross-cutting metadata (actor identity, correlation ID, extra metadata) |

[`Aggregate`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.Aggregate.html
[`AggregateStore`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateStore.html
[`AggregateStoreBuilder`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateStoreBuilder.html
[`Projection`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.Projection.html
[`ProcessManager`]: https://docs.rs/eventfold-es/latest/eventfold_es/trait.ProcessManager.html
[`CommandBus`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.CommandBus.html
[`AggregateHandle`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.AggregateHandle.html
[`CommandContext`]: https://docs.rs/eventfold-es/latest/eventfold_es/struct.CommandContext.html

## Architecture

```
tokio async world                     blocking OS threads
=====================                 ====================
AggregateStore                        Actor (1 per instance)
  handle cache (Arc<RwLock>)  ----->    EventWriter + View<A>
AggregateHandle (Clone)                 sequential message loop
CommandBus                              idle timeout -> exit

ProjectionRunner                      (no dedicated thread)
ProcessManagerRunner                    blocking I/O under std::sync::Mutex
```

Each aggregate instance runs on a **dedicated blocking thread**, not a tokio task. The actor exclusively owns the `EventWriter` (file lock) and processes commands sequentially. This guarantees single-writer consistency without optimistic concurrency retries.

Idle actors shut down after a configurable timeout (default 5 minutes), releasing their file lock. The next `store.get()` transparently re-spawns the actor and recovers state from disk.

## Storage Layout

All data lives under a single base directory:

```
<base_dir>/
    streams/<aggregate_type>/<instance_id>/
        app.jsonl                   # eventfold append-only event log
        views/state.snapshot.json   # eventfold view snapshot
    projections/<name>/
        checkpoint.json             # projection state + per-stream cursors
    process_managers/<name>/
        checkpoint.json             # PM state + per-stream cursors
        dead_letters.jsonl          # failed dispatches
    meta/
        streams.jsonl               # stream registry
```

Event logs are plain JSONL, fully compatible with standard Unix tools and the `eventfold` CLI.

## Configuration

```rust
let store = AggregateStore::builder("/tmp/my-app")
    .projection::<MyProjection>()
    .process_manager::<MySaga>()
    .aggregate_type::<TargetAggregate>()  // dispatch target for process managers
    .idle_timeout(Duration::from_secs(600))
    .open()
    .await?;
```

- **`projection::<P>()`** -- register a read model
- **`process_manager::<PM>()`** -- register a workflow coordinator
- **`aggregate_type::<A>()`** -- register a dispatch target (requires `A::Command: DeserializeOwned`)
- **`idle_timeout(dur)`** -- set actor eviction timeout (default: 5 min)

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

This maps cleanly to eventfold's `event_type` + `data` fields.

## License

MIT
