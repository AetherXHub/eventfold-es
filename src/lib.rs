//! Event-sourcing primitives built on top of `eventfold` aggregates.

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

pub use command::{CommandContext, CommandEnvelope};
pub use error::{ExecuteError, StateError};
pub use process_manager::{DispatchError, ProcessManager, ProcessManagerReport};
pub use projection::Projection;
pub use storage::StreamLayout;
pub use store::{AggregateStore, AggregateStoreBuilder};
