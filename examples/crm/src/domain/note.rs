//! Note aggregate -- freeform notes linked to contacts or deals.
//!
//! Notes are simple text blobs. They can be edited but never deleted.

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A CRM note (text annotation).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Note body text.
    pub body: String,
    /// Optional linked contact instance ID.
    pub contact_id: Option<String>,
    /// Optional linked deal instance ID.
    pub deal_id: Option<String>,
    /// Optional linked company instance ID.
    pub company_id: Option<String>,
    /// Optional linked task instance ID.
    pub task_id: Option<String>,
    /// Whether the note has been created.
    pub created: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Note`] aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NoteCommand {
    /// Create a new note.
    Create {
        body: String,
        contact_id: Option<String>,
        deal_id: Option<String>,
        company_id: Option<String>,
        task_id: Option<String>,
    },
    /// Edit the note body.
    Edit { body: String },
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Note`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum NoteEvent {
    /// A new note was created.
    Created {
        body: String,
        contact_id: Option<String>,
        deal_id: Option<String>,
        company_id: Option<String>,
        task_id: Option<String>,
    },
    /// The note body was edited.
    Edited { body: String },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when handling a [`NoteCommand`].
#[derive(Debug, thiserror::Error)]
pub enum NoteError {
    /// The note body must not be empty.
    #[error("note body must not be empty")]
    EmptyBody,
    /// Attempted to create a note that already exists.
    #[error("note already exists")]
    AlreadyExists,
    /// Attempted to modify a note that has not been created.
    #[error("note does not exist")]
    NotCreated,
}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Note {
    const AGGREGATE_TYPE: &'static str = "note";
    type Command = NoteCommand;
    type DomainEvent = NoteEvent;
    type Error = NoteError;

    fn handle(&self, cmd: NoteCommand) -> Result<Vec<NoteEvent>, NoteError> {
        match cmd {
            NoteCommand::Create {
                body,
                contact_id,
                deal_id,
                company_id,
                task_id,
            } => {
                if body.trim().is_empty() {
                    return Err(NoteError::EmptyBody);
                }
                if self.created {
                    return Err(NoteError::AlreadyExists);
                }
                Ok(vec![NoteEvent::Created {
                    body,
                    contact_id,
                    deal_id,
                    company_id,
                    task_id,
                }])
            }
            NoteCommand::Edit { body } => {
                if !self.created {
                    return Err(NoteError::NotCreated);
                }
                if body.trim().is_empty() {
                    return Err(NoteError::EmptyBody);
                }
                Ok(vec![NoteEvent::Edited { body }])
            }
        }
    }

    fn apply(mut self, event: &NoteEvent) -> Self {
        match event {
            NoteEvent::Created {
                body,
                contact_id,
                deal_id,
                company_id,
                task_id,
            } => {
                self.body = body.clone();
                self.contact_id = contact_id.clone();
                self.deal_id = deal_id.clone();
                self.company_id = company_id.clone();
                self.task_id = task_id.clone();
                self.created = true;
            }
            NoteEvent::Edited { body } => {
                self.body = body.clone();
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

    fn created_note() -> Note {
        let n = Note::default();
        let events = n
            .handle(NoteCommand::Create {
                body: "Initial note".into(),
                contact_id: Some("contact-1".into()),
                deal_id: None,
                company_id: Some("comp-1".into()),
                task_id: None,
            })
            .expect("create should succeed");
        events.into_iter().fold(Note::default(), |s, e| s.apply(&e))
    }

    #[test]
    fn create_note() {
        let n = created_note();
        assert_eq!(n.body, "Initial note");
        assert_eq!(n.contact_id.as_deref(), Some("contact-1"));
        assert_eq!(n.company_id.as_deref(), Some("comp-1"));
        assert!(n.task_id.is_none());
        assert!(n.created);
    }

    #[test]
    fn reject_empty_body() {
        let n = Note::default();
        let err = n
            .handle(NoteCommand::Create {
                body: "  ".into(),
                contact_id: None,
                deal_id: None,
                company_id: None,
                task_id: None,
            })
            .unwrap_err();
        assert!(matches!(err, NoteError::EmptyBody));
    }

    #[test]
    fn reject_double_create() {
        let n = created_note();
        let err = n
            .handle(NoteCommand::Create {
                body: "Other".into(),
                contact_id: None,
                deal_id: None,
                company_id: None,
                task_id: None,
            })
            .unwrap_err();
        assert!(matches!(err, NoteError::AlreadyExists));
    }

    #[test]
    fn edit_note() {
        let n = created_note();
        let events = n
            .handle(NoteCommand::Edit {
                body: "Updated note".into(),
            })
            .expect("edit ok");
        let n = events.into_iter().fold(n, |s, e| s.apply(&e));
        assert_eq!(n.body, "Updated note");
    }

    #[test]
    fn reject_edit_empty_body() {
        let n = created_note();
        let err = n.handle(NoteCommand::Edit { body: "".into() }).unwrap_err();
        assert!(matches!(err, NoteError::EmptyBody));
    }

    #[test]
    fn reject_edit_uncreated() {
        let n = Note::default();
        let err = n
            .handle(NoteCommand::Edit {
                body: "test".into(),
            })
            .unwrap_err();
        assert!(matches!(err, NoteError::NotCreated));
    }
}
