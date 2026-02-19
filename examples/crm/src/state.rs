//! Shared application state managed by Tauri.

use eventfold_es::{AggregateStore, CommandBus};

/// Application state holding the event store and command bus.
///
/// Registered directly with `app.manage()` -- no `Mutex` needed because
/// both `AggregateStore` (internally `Arc`-wrapped) and `CommandBus`
/// (all trait objects are `Send + Sync`) are `Send + Sync`. Tauri provides
/// `&AppState` to command handlers, which is sufficient since all operations
/// take `&self`.
pub struct AppState {
    /// The central event store (aggregates, projections, process managers).
    pub store: AggregateStore,
    /// Typed command router for dispatching commands to aggregates.
    pub bus: CommandBus,
}
