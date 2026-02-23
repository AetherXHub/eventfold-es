//! Embedded event-sourcing framework built on top of [`eventfold`].
//!
//! `eventfold-es` provides the building blocks for event-sourced applications:
//! aggregates, projections, process managers, and a typed command bus. All
//! state is persisted to disk via `eventfold`'s append-only JSONL logs --
//! no external database required.
//!
//! # Key Types
//!
//! | Type | Role |
//! |------|------|
//! | [`Aggregate`] | Domain model: handles commands, emits events, folds state |
//! | [`AggregateStore`] | Central registry: spawns actors, caches handles, runs projections |
//! | [`Projection`] | Cross-stream read model built from events |
//! | [`ProcessManager`] | Cross-aggregate workflow that reacts to events with commands |
//! | [`CommandBus`] | Typed command router keyed by `TypeId` |
//! | [`AggregateHandle`] | Async handle to a running aggregate actor |
//!
//! # Quick Start
//!
//! ```no_run
//! use eventfold_es::{
//!     Aggregate, AggregateStore, CommandContext,
//! };
//! use serde::{Deserialize, Serialize};
//!
//! // 1. Define your aggregate.
//! #[derive(Debug, Clone, Default, Serialize, Deserialize)]
//! struct Counter { value: u64 }
//!
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! #[serde(tag = "type", content = "data")]
//! enum CounterEvent { Incremented }
//!
//! #[derive(Debug, thiserror::Error)]
//! enum CounterError {}
//!
//! impl Aggregate for Counter {
//!     const AGGREGATE_TYPE: &'static str = "counter";
//!     type Command = String;  // simplified for example
//!     type DomainEvent = CounterEvent;
//!     type Error = CounterError;
//!
//!     fn handle(&self, _cmd: String) -> Result<Vec<CounterEvent>, CounterError> {
//!         Ok(vec![CounterEvent::Incremented])
//!     }
//!     fn apply(mut self, _event: &CounterEvent) -> Self {
//!         self.value += 1;
//!         self
//!     }
//! }
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // 2. Open the store and send commands.
//! let store = AggregateStore::open("/tmp/my-app").await?;
//! let handle = store.get::<Counter>("counter-1").await?;
//! handle.execute("go".into(), CommandContext::default()).await?;
//!
//! let state = handle.state().await?;
//! assert_eq!(state.value, 1);
//! # Ok(())
//! # }
//! ```
//!
//! See `examples/counter.rs` for a self-contained runnable example that
//! demonstrates aggregates, projections, and the command bus.

mod actor;
pub use actor::{AggregateHandle, spawn_actor};
mod aggregate;
pub use aggregate::{Aggregate, reducer, to_eventfold_event};
mod command;
mod error;
mod process_manager;
mod projection;
mod storage;
mod store;

pub use command::{CommandBus, CommandContext, CommandEnvelope};
pub use error::{DispatchError, ExecuteError, StateError};
pub use process_manager::{ProcessManager, ProcessManagerReport};
pub use projection::Projection;
pub use storage::StreamLayout;
pub use store::{AggregateStore, AggregateStoreBuilder, InjectOptions};
