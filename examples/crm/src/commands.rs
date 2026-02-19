//! Tauri command handlers exposing the CRM API to the frontend.
//!
//! Every write command dispatches through the [`CommandBus`] and then runs
//! process managers. Read commands query aggregate state or projections
//! directly.
//!
//! All handlers take `State<'_, AppState>` directly (no `Mutex`) because
//! both `AggregateStore` and `CommandBus` are `Send + Sync` and only
//! require `&self` for all operations.

use tauri::State;

use crate::domain::{
    Company, CompanyCommand, Contact, ContactCommand, Deal, DealCommand, DealStage, Note,
    NoteCommand, Settings, SettingsCommand, Task, TaskCommand,
};
use crate::error::AppError;
use crate::projections::{ActivityFeed, PipelineSummary};
use crate::state::AppState;

use eventfold_es::CommandContext;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a default command context with actor = "crm-user".
fn ctx() -> CommandContext {
    CommandContext::default().with_actor("crm-user")
}

// ---------------------------------------------------------------------------
// Contact commands
// ---------------------------------------------------------------------------

/// Create a new contact.
#[tauri::command]
pub async fn create_contact(
    id: String,
    name: String,
    email: String,
    phone: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::Create { name, email, phone }, ctx())
        .await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Update an existing contact's details.
#[tauri::command]
pub async fn update_contact(
    id: String,
    name: String,
    email: String,
    phone: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::Update { name, email, phone }, ctx())
        .await?;
    Ok(())
}

/// Archive (soft-delete) a contact.
#[tauri::command]
pub async fn archive_contact(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::Archive, ctx())
        .await?;
    Ok(())
}

/// Link a contact to a company.
#[tauri::command]
pub async fn link_contact_company(
    id: String,
    company_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::LinkCompany { company_id }, ctx())
        .await?;
    Ok(())
}

/// Add a tag to a contact.
#[tauri::command]
pub async fn add_contact_tag(
    id: String,
    tag: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::AddTag { tag }, ctx())
        .await?;
    Ok(())
}

/// Remove a tag from a contact.
#[tauri::command]
pub async fn remove_contact_tag(
    id: String,
    tag: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, ContactCommand::RemoveTag { tag }, ctx())
        .await?;
    Ok(())
}

/// Get a single contact by ID.
#[tauri::command]
pub async fn get_contact(
    id: String,
    state: State<'_, AppState>,
) -> Result<ContactWithId, AppError> {
    let handle = state.store.get::<Contact>(&id).await?;
    let c = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    if !c.created {
        return Err(AppError::NotFound(format!("contact {id}")));
    }
    Ok(ContactWithId {
        id,
        name: c.name,
        email: c.email,
        phone: c.phone,
        company_id: c.company_id,
        tags: c.tags,
    })
}

/// List all contacts (excludes archived).
#[tauri::command]
pub async fn list_contacts(state: State<'_, AppState>) -> Result<Vec<ContactWithId>, AppError> {
    let ids = state.store.list::<Contact>().await?;
    let mut contacts = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = state.store.get::<Contact>(id).await?;
        let c = handle
            .state()
            .await
            .map_err(|e| AppError::Store(e.to_string()))?;
        if c.created && !c.archived {
            contacts.push(ContactWithId {
                id: id.clone(),
                name: c.name,
                email: c.email,
                phone: c.phone,
                company_id: c.company_id,
                tags: c.tags,
            });
        }
    }
    Ok(contacts)
}

/// Contact state bundled with its instance ID for frontend display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContactWithId {
    /// Instance ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Phone number.
    pub phone: String,
    /// Optional linked company ID.
    pub company_id: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Company commands
// ---------------------------------------------------------------------------

/// Create a new company.
#[tauri::command]
pub async fn create_company(
    id: String,
    name: String,
    industry: String,
    website: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(
            &id,
            CompanyCommand::Create {
                name,
                industry,
                website,
            },
            ctx(),
        )
        .await?;
    Ok(())
}

/// Update an existing company.
#[tauri::command]
pub async fn update_company(
    id: String,
    name: String,
    industry: String,
    website: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(
            &id,
            CompanyCommand::Update {
                name,
                industry,
                website,
            },
            ctx(),
        )
        .await?;
    Ok(())
}

/// Archive (soft-delete) a company.
#[tauri::command]
pub async fn archive_company(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, CompanyCommand::Archive, ctx())
        .await?;
    Ok(())
}

/// Get a single company by ID.
#[tauri::command]
pub async fn get_company(
    id: String,
    state: State<'_, AppState>,
) -> Result<CompanyWithId, AppError> {
    let handle = state.store.get::<Company>(&id).await?;
    let c = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    if !c.created {
        return Err(AppError::NotFound(format!("company {id}")));
    }
    Ok(CompanyWithId {
        id,
        name: c.name,
        industry: c.industry,
        website: c.website,
    })
}

/// List all companies (excludes archived).
#[tauri::command]
pub async fn list_companies(state: State<'_, AppState>) -> Result<Vec<CompanyWithId>, AppError> {
    let ids = state.store.list::<Company>().await?;
    let mut companies = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = state.store.get::<Company>(id).await?;
        let c = handle
            .state()
            .await
            .map_err(|e| AppError::Store(e.to_string()))?;
        if c.created && !c.archived {
            companies.push(CompanyWithId {
                id: id.clone(),
                name: c.name,
                industry: c.industry,
                website: c.website,
            });
        }
    }
    Ok(companies)
}

/// Company state bundled with its instance ID for frontend display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompanyWithId {
    /// Instance ID.
    pub id: String,
    /// Company name.
    pub name: String,
    /// Industry.
    pub industry: String,
    /// Optional website URL.
    pub website: Option<String>,
}

// ---------------------------------------------------------------------------
// Deal commands
// ---------------------------------------------------------------------------

/// Create a new deal in the Prospect stage.
#[tauri::command]
pub async fn create_deal(
    id: String,
    title: String,
    company_id: String,
    contact_id: String,
    value: u64,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(
            &id,
            DealCommand::Create {
                title,
                company_id,
                contact_id,
                value,
            },
            ctx(),
        )
        .await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Advance a deal to the next pipeline stage.
#[tauri::command]
pub async fn advance_deal(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.bus.dispatch(&id, DealCommand::Advance, ctx()).await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Set the monetary value of a deal.
#[tauri::command]
pub async fn set_deal_value(
    id: String,
    value: u64,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, DealCommand::SetValue { value }, ctx())
        .await?;
    Ok(())
}

/// Close a deal as won.
#[tauri::command]
pub async fn win_deal(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.bus.dispatch(&id, DealCommand::Win, ctx()).await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Close a deal as lost.
#[tauri::command]
pub async fn lose_deal(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.bus.dispatch(&id, DealCommand::Lose, ctx()).await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Get a single deal by ID.
#[tauri::command]
pub async fn get_deal(id: String, state: State<'_, AppState>) -> Result<DealWithId, AppError> {
    let handle = state.store.get::<Deal>(&id).await?;
    let d = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    if !d.created {
        return Err(AppError::NotFound(format!("deal {id}")));
    }
    Ok(DealWithId {
        id,
        title: d.title,
        company_id: d.company_id,
        contact_id: d.contact_id,
        stage: d.stage,
        value: d.value,
        closed: d.closed,
    })
}

/// List all deals (including closed).
#[tauri::command]
pub async fn list_deals(state: State<'_, AppState>) -> Result<Vec<DealWithId>, AppError> {
    let ids = state.store.list::<Deal>().await?;
    let mut deals = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = state.store.get::<Deal>(id).await?;
        let d = handle
            .state()
            .await
            .map_err(|e| AppError::Store(e.to_string()))?;
        if d.created {
            deals.push(DealWithId {
                id: id.clone(),
                title: d.title,
                company_id: d.company_id,
                contact_id: d.contact_id,
                stage: d.stage,
                value: d.value,
                closed: d.closed,
            });
        }
    }
    Ok(deals)
}

/// Deal state bundled with its instance ID for frontend display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DealWithId {
    /// Instance ID.
    pub id: String,
    /// Deal title.
    pub title: String,
    /// Company instance ID.
    pub company_id: String,
    /// Contact instance ID.
    pub contact_id: String,
    /// Current pipeline stage.
    pub stage: DealStage,
    /// Monetary value in cents.
    pub value: u64,
    /// Whether the deal is closed.
    pub closed: bool,
}

// ---------------------------------------------------------------------------
// Task commands
// ---------------------------------------------------------------------------

/// Create a new task.
#[tauri::command]
pub async fn create_task(
    id: String,
    title: String,
    description: String,
    deal_id: Option<String>,
    contact_id: Option<String>,
    due: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(
            &id,
            TaskCommand::Create {
                title,
                description,
                deal_id,
                contact_id,
                due,
            },
            ctx(),
        )
        .await?;
    Ok(())
}

/// Mark a task as completed.
#[tauri::command]
pub async fn complete_task(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, TaskCommand::Complete, ctx())
        .await?;
    Ok(())
}

/// Reopen a completed task.
#[tauri::command]
pub async fn reopen_task(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.bus.dispatch(&id, TaskCommand::Reopen, ctx()).await?;
    Ok(())
}

/// Update a task's due date.
#[tauri::command]
pub async fn update_task_due(
    id: String,
    due: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, TaskCommand::UpdateDue { due }, ctx())
        .await?;
    Ok(())
}

/// Get a single task by ID.
#[tauri::command]
pub async fn get_task(id: String, state: State<'_, AppState>) -> Result<Task, AppError> {
    let handle = state.store.get::<Task>(&id).await?;
    let task = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    if !task.created {
        return Err(AppError::NotFound(format!("task {id}")));
    }
    Ok(task)
}

/// List all tasks.
#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<TaskWithId>, AppError> {
    let ids = state.store.list::<Task>().await?;
    let mut tasks = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = state.store.get::<Task>(id).await?;
        let t = handle
            .state()
            .await
            .map_err(|e| AppError::Store(e.to_string()))?;
        if t.created {
            tasks.push(TaskWithId {
                id: id.clone(),
                title: t.title,
                description: t.description,
                deal_id: t.deal_id,
                contact_id: t.contact_id,
                done: t.done,
                due: t.due,
            });
        }
    }
    Ok(tasks)
}

/// Task state bundled with its instance ID for frontend display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskWithId {
    /// Instance ID.
    pub id: String,
    /// Task title.
    pub title: String,
    /// Description text.
    pub description: String,
    /// Optional linked deal ID.
    pub deal_id: Option<String>,
    /// Optional linked contact ID.
    pub contact_id: Option<String>,
    /// Whether the task is done.
    pub done: bool,
    /// Optional due date.
    pub due: Option<String>,
}

// ---------------------------------------------------------------------------
// Note commands
// ---------------------------------------------------------------------------

/// Create a new note.
#[tauri::command]
pub async fn create_note(
    id: String,
    body: String,
    contact_id: Option<String>,
    deal_id: Option<String>,
    company_id: Option<String>,
    task_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(
            &id,
            NoteCommand::Create {
                body,
                contact_id,
                deal_id,
                company_id,
                task_id,
            },
            ctx(),
        )
        .await?;
    state.store.run_process_managers().await?;
    Ok(())
}

/// Edit an existing note's body.
#[tauri::command]
pub async fn edit_note(
    id: String,
    body: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .bus
        .dispatch(&id, NoteCommand::Edit { body }, ctx())
        .await?;
    Ok(())
}

/// Get a single note by ID.
#[tauri::command]
pub async fn get_note(id: String, state: State<'_, AppState>) -> Result<Note, AppError> {
    let handle = state.store.get::<Note>(&id).await?;
    let note = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    if !note.created {
        return Err(AppError::NotFound(format!("note {id}")));
    }
    Ok(note)
}

/// List all notes.
#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>) -> Result<Vec<NoteWithId>, AppError> {
    let ids = state.store.list::<Note>().await?;
    let mut notes = Vec::with_capacity(ids.len());
    for id in &ids {
        let handle = state.store.get::<Note>(id).await?;
        let n = handle
            .state()
            .await
            .map_err(|e| AppError::Store(e.to_string()))?;
        if n.created {
            notes.push(NoteWithId {
                id: id.clone(),
                body: n.body,
                contact_id: n.contact_id,
                deal_id: n.deal_id,
                company_id: n.company_id,
                task_id: n.task_id,
            });
        }
    }
    Ok(notes)
}

/// Note state bundled with its instance ID for frontend display.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NoteWithId {
    /// Instance ID.
    pub id: String,
    /// Note body text.
    pub body: String,
    /// Optional linked contact ID.
    pub contact_id: Option<String>,
    /// Optional linked deal ID.
    pub deal_id: Option<String>,
    /// Optional linked company ID.
    pub company_id: Option<String>,
    /// Optional linked task ID.
    pub task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Settings commands
// ---------------------------------------------------------------------------

/// Get the current application settings.
///
/// Returns the default state (light mode) if no settings events have been
/// recorded yet, since the default is a valid "no preference set" state.
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    let handle = state.store.get::<Settings>("global").await?;
    let settings = handle
        .state()
        .await
        .map_err(|e| AppError::Store(e.to_string()))?;
    Ok(settings)
}

/// Set the dark mode preference.
#[tauri::command]
pub async fn set_dark_mode(enabled: bool, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .bus
        .dispatch("global", SettingsCommand::SetDarkMode { enabled }, ctx())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Projection queries
// ---------------------------------------------------------------------------

/// Get the pipeline summary for the dashboard.
#[tauri::command]
pub async fn get_pipeline_summary(state: State<'_, AppState>) -> Result<PipelineSummary, AppError> {
    let proj = state.store.projection::<PipelineSummary>()?;
    Ok(proj)
}

/// Get the activity feed for the dashboard.
#[tauri::command]
pub async fn get_activity_feed(state: State<'_, AppState>) -> Result<ActivityFeed, AppError> {
    let proj = state.store.projection::<ActivityFeed>()?;
    Ok(proj)
}
