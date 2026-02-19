//! EventFold CRM -- Tauri v2 desktop application.
//!
//! Demonstrates every eventfold-es feature (aggregates, projections, process
//! managers, CommandBus) in a sales-pipeline CRM domain.

mod commands;
pub mod domain;
mod error;
pub mod projections;
mod state;
pub mod workflows;

use std::time::Duration;

use eventfold_es::{AggregateStore, CommandBus};
use tauri::Manager;

use crate::domain::{Company, Contact, Deal, Note, Settings, Task};
use crate::projections::{ActivityFeed, PipelineSummary};
use crate::state::AppState;
use crate::workflows::DealTaskCreator;

/// Launch the Tauri application.
///
/// Builds the event store with projections and process managers, registers
/// all aggregate types on the command bus, then starts the Tauri window.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Resolve the platform-appropriate data directory.
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");

            // Build the store on a dedicated tokio runtime (Tauri setup is
            // synchronous, so we need a blocking bridge).
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

            let store = rt
                .block_on(async {
                    AggregateStore::builder(&data_dir)
                        .projection::<PipelineSummary>()
                        .projection::<ActivityFeed>()
                        .process_manager::<DealTaskCreator>()
                        .aggregate_type::<Task>() // dispatch target for PM
                        .idle_timeout(Duration::from_secs(300))
                        .open()
                        .await
                })
                .expect("failed to open aggregate store");

            let mut bus = CommandBus::new(store.clone());
            bus.register::<Contact>();
            bus.register::<Company>();
            bus.register::<Deal>();
            bus.register::<Task>();
            bus.register::<Note>();
            bus.register::<Settings>();

            app.manage(AppState { store, bus });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Contacts
            commands::create_contact,
            commands::update_contact,
            commands::archive_contact,
            commands::link_contact_company,
            commands::add_contact_tag,
            commands::remove_contact_tag,
            commands::get_contact,
            commands::list_contacts,
            // Companies
            commands::create_company,
            commands::update_company,
            commands::archive_company,
            commands::get_company,
            commands::list_companies,
            // Deals
            commands::create_deal,
            commands::advance_deal,
            commands::set_deal_value,
            commands::win_deal,
            commands::lose_deal,
            commands::get_deal,
            commands::list_deals,
            // Tasks
            commands::create_task,
            commands::complete_task,
            commands::reopen_task,
            commands::update_task_due,
            commands::get_task,
            commands::list_tasks,
            // Notes
            commands::create_note,
            commands::edit_note,
            commands::get_note,
            commands::list_notes,
            // Settings
            commands::get_settings,
            commands::set_dark_mode,
            // Projections
            commands::get_pipeline_summary,
            commands::get_activity_feed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
