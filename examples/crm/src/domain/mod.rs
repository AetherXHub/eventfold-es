//! CRM domain aggregates.
//!
//! Each module defines one aggregate (state, commands, events, business rules).

pub mod company;
pub mod contact;
pub mod deal;
pub mod note;
pub mod settings;
pub mod task;

pub use company::{Company, CompanyCommand, CompanyError, CompanyEvent};
pub use contact::{Contact, ContactCommand, ContactError, ContactEvent};
pub use deal::{Deal, DealCommand, DealError, DealEvent, DealStage};
pub use note::{Note, NoteCommand, NoteError, NoteEvent};
pub use settings::{Settings, SettingsCommand, SettingsError, SettingsEvent};
pub use task::{Task, TaskCommand, TaskError, TaskEvent};
