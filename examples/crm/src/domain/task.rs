//! Task aggregate -- to-do items linked to deals or contacts.
//!
//! Tasks can be completed and reopened. An optional due date is stored as an
//! ISO 8601 date string (e.g. "2026-03-15").

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A CRM task (to-do item).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Task title.
    pub title: String,
    /// Optional longer description.
    pub description: String,
    /// Optional linked deal instance ID.
    pub deal_id: Option<String>,
    /// Optional linked contact instance ID.
    pub contact_id: Option<String>,
    /// Whether this task has been completed.
    pub done: bool,
    /// Optional due date (ISO 8601 date string).
    pub due: Option<String>,
    /// Whether the task has been created.
    pub created: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Task`] aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TaskCommand {
    /// Create a new task.
    Create {
        title: String,
        description: String,
        deal_id: Option<String>,
        contact_id: Option<String>,
        due: Option<String>,
    },
    /// Mark the task as done.
    Complete,
    /// Reopen a completed task.
    Reopen,
    /// Update the due date.
    UpdateDue { due: Option<String> },
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Task`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TaskEvent {
    /// A new task was created.
    Created {
        title: String,
        description: String,
        deal_id: Option<String>,
        contact_id: Option<String>,
        due: Option<String>,
    },
    /// The task was marked as done.
    Completed,
    /// The task was reopened.
    Reopened,
    /// The due date was updated.
    DueUpdated { due: Option<String> },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when handling a [`TaskCommand`].
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// The task title must not be empty.
    #[error("task title must not be empty")]
    EmptyTitle,
    /// Attempted to create a task that already exists.
    #[error("task already exists")]
    AlreadyExists,
    /// Attempted to complete a task that is already done.
    #[error("task is already completed")]
    AlreadyDone,
    /// Attempted to reopen a task that is not done.
    #[error("task is not completed")]
    NotDone,
    /// Attempted to modify a task that has not been created.
    #[error("task does not exist")]
    NotCreated,
}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Task {
    const AGGREGATE_TYPE: &'static str = "task";
    type Command = TaskCommand;
    type DomainEvent = TaskEvent;
    type Error = TaskError;

    fn handle(&self, cmd: TaskCommand) -> Result<Vec<TaskEvent>, TaskError> {
        match cmd {
            TaskCommand::Create {
                title,
                description,
                deal_id,
                contact_id,
                due,
            } => {
                if title.trim().is_empty() {
                    return Err(TaskError::EmptyTitle);
                }
                if self.created {
                    return Err(TaskError::AlreadyExists);
                }
                Ok(vec![TaskEvent::Created {
                    title,
                    description,
                    deal_id,
                    contact_id,
                    due,
                }])
            }
            TaskCommand::Complete => {
                if !self.created {
                    return Err(TaskError::NotCreated);
                }
                if self.done {
                    return Err(TaskError::AlreadyDone);
                }
                Ok(vec![TaskEvent::Completed])
            }
            TaskCommand::Reopen => {
                if !self.created {
                    return Err(TaskError::NotCreated);
                }
                if !self.done {
                    return Err(TaskError::NotDone);
                }
                Ok(vec![TaskEvent::Reopened])
            }
            TaskCommand::UpdateDue { due } => {
                if !self.created {
                    return Err(TaskError::NotCreated);
                }
                Ok(vec![TaskEvent::DueUpdated { due }])
            }
        }
    }

    fn apply(mut self, event: &TaskEvent) -> Self {
        match event {
            TaskEvent::Created {
                title,
                description,
                deal_id,
                contact_id,
                due,
            } => {
                self.title = title.clone();
                self.description = description.clone();
                self.deal_id = deal_id.clone();
                self.contact_id = contact_id.clone();
                self.due = due.clone();
                self.created = true;
            }
            TaskEvent::Completed => {
                self.done = true;
            }
            TaskEvent::Reopened => {
                self.done = false;
            }
            TaskEvent::DueUpdated { due } => {
                self.due = due.clone();
            }
        }
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn created_task() -> Task {
        let t = Task::default();
        let events = t
            .handle(TaskCommand::Create {
                title: "Follow up".into(),
                description: "Call the client".into(),
                deal_id: Some("deal-1".into()),
                contact_id: None,
                due: Some("2026-03-15".into()),
            })
            .expect("create should succeed");
        events.into_iter().fold(Task::default(), |s, e| s.apply(&e))
    }

    #[test]
    fn create_task() {
        let t = created_task();
        assert_eq!(t.title, "Follow up");
        assert!(!t.done);
        assert!(t.created);
        assert_eq!(t.due.as_deref(), Some("2026-03-15"));
    }

    #[test]
    fn reject_empty_title() {
        let t = Task::default();
        let err = t
            .handle(TaskCommand::Create {
                title: "".into(),
                description: "".into(),
                deal_id: None,
                contact_id: None,
                due: None,
            })
            .unwrap_err();
        assert!(matches!(err, TaskError::EmptyTitle));
    }

    #[test]
    fn complete_and_reopen() {
        let t = created_task();
        let events = t.handle(TaskCommand::Complete).expect("complete ok");
        let t = events.into_iter().fold(t, |s, e| s.apply(&e));
        assert!(t.done);

        // Cannot complete again.
        let err = t.handle(TaskCommand::Complete).unwrap_err();
        assert!(matches!(err, TaskError::AlreadyDone));

        // Reopen.
        let events = t.handle(TaskCommand::Reopen).expect("reopen ok");
        let t = events.into_iter().fold(t, |s, e| s.apply(&e));
        assert!(!t.done);

        // Cannot reopen when not done.
        let err = t.handle(TaskCommand::Reopen).unwrap_err();
        assert!(matches!(err, TaskError::NotDone));
    }

    #[test]
    fn update_due_date() {
        let t = created_task();
        let events = t
            .handle(TaskCommand::UpdateDue {
                due: Some("2026-04-01".into()),
            })
            .expect("update due ok");
        let t = events.into_iter().fold(t, |s, e| s.apply(&e));
        assert_eq!(t.due.as_deref(), Some("2026-04-01"));
    }
}
