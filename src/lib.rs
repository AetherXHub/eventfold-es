//! Event-sourcing primitives built on top of `eventfold` aggregates.

mod actor;
pub use actor::{AggregateHandle, spawn_actor};
mod aggregate;
pub use aggregate::{Aggregate, reducer, to_eventfold_event};
mod command;
mod error;
mod projection;
mod storage;
mod store;

pub use command::CommandContext;
pub use error::{ExecuteError, StateError};
pub use projection::Projection;
pub use storage::StreamLayout;
pub use store::{AggregateStore, AggregateStoreBuilder};
