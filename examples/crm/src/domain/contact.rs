//! Contact aggregate -- people in the CRM.
//!
//! Contacts represent individuals who may be linked to a company and tagged
//! for filtering. A contact can be archived (soft-deleted) but not un-archived.

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A CRM contact (person).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
    /// Phone number.
    pub phone: String,
    /// Optional linked company instance ID.
    pub company_id: Option<String>,
    /// Free-form tags for filtering.
    pub tags: Vec<String>,
    /// Whether this contact has been archived.
    pub archived: bool,
    /// Whether the contact has been created (used to guard double-create).
    pub created: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Contact`] aggregate.
///
/// Each variant carries the data needed for validation and event emission.
/// Implements `Deserialize` so that process managers can dispatch commands
/// via JSON `CommandEnvelope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ContactCommand {
    /// Create a new contact with the given details.
    Create {
        name: String,
        email: String,
        phone: String,
    },
    /// Update mutable fields on an existing contact.
    Update {
        name: String,
        email: String,
        phone: String,
    },
    /// Soft-delete the contact.
    Archive,
    /// Link this contact to a company.
    LinkCompany { company_id: String },
    /// Remove the company link.
    Unlink,
    /// Add a tag to the contact.
    AddTag { tag: String },
    /// Remove a tag from the contact.
    RemoveTag { tag: String },
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Contact`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ContactEvent {
    /// A new contact was created.
    Created {
        name: String,
        email: String,
        phone: String,
    },
    /// Contact fields were updated.
    Updated {
        name: String,
        email: String,
        phone: String,
    },
    /// The contact was archived.
    Archived,
    /// A company was linked to this contact.
    CompanyLinked { company_id: String },
    /// The company link was removed.
    CompanyUnlinked,
    /// A tag was added.
    TagAdded { tag: String },
    /// A tag was removed.
    TagRemoved { tag: String },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when handling a [`ContactCommand`].
#[derive(Debug, thiserror::Error)]
pub enum ContactError {
    /// The contact name must not be empty.
    #[error("contact name must not be empty")]
    EmptyName,
    /// Attempted to create a contact that already exists.
    #[error("contact already exists")]
    AlreadyExists,
    /// Attempted to modify an archived contact.
    #[error("contact is archived")]
    Archived,
    /// Attempted to archive a contact that is already archived.
    #[error("contact is already archived")]
    AlreadyArchived,
    /// Attempted to add a duplicate tag.
    #[error("tag already exists: {0}")]
    DuplicateTag(String),
    /// Attempted to remove a tag that does not exist.
    #[error("tag not found: {0}")]
    TagNotFound(String),
    /// Attempted to modify a contact that has not been created.
    #[error("contact does not exist")]
    NotCreated,
}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Contact {
    const AGGREGATE_TYPE: &'static str = "contact";
    type Command = ContactCommand;
    type DomainEvent = ContactEvent;
    type Error = ContactError;

    fn handle(&self, cmd: ContactCommand) -> Result<Vec<ContactEvent>, ContactError> {
        match cmd {
            ContactCommand::Create { name, email, phone } => {
                if name.trim().is_empty() {
                    return Err(ContactError::EmptyName);
                }
                if self.created {
                    return Err(ContactError::AlreadyExists);
                }
                Ok(vec![ContactEvent::Created { name, email, phone }])
            }
            ContactCommand::Update { name, email, phone } => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::Archived);
                }
                if name.trim().is_empty() {
                    return Err(ContactError::EmptyName);
                }
                Ok(vec![ContactEvent::Updated { name, email, phone }])
            }
            ContactCommand::Archive => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::AlreadyArchived);
                }
                Ok(vec![ContactEvent::Archived])
            }
            ContactCommand::LinkCompany { company_id } => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::Archived);
                }
                Ok(vec![ContactEvent::CompanyLinked { company_id }])
            }
            ContactCommand::Unlink => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::Archived);
                }
                Ok(vec![ContactEvent::CompanyUnlinked])
            }
            ContactCommand::AddTag { tag } => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::Archived);
                }
                if self.tags.contains(&tag) {
                    return Err(ContactError::DuplicateTag(tag));
                }
                Ok(vec![ContactEvent::TagAdded { tag }])
            }
            ContactCommand::RemoveTag { tag } => {
                if !self.created {
                    return Err(ContactError::NotCreated);
                }
                if self.archived {
                    return Err(ContactError::Archived);
                }
                if !self.tags.contains(&tag) {
                    return Err(ContactError::TagNotFound(tag));
                }
                Ok(vec![ContactEvent::TagRemoved { tag }])
            }
        }
    }

    fn apply(mut self, event: &ContactEvent) -> Self {
        match event {
            ContactEvent::Created { name, email, phone } => {
                self.name = name.clone();
                self.email = email.clone();
                self.phone = phone.clone();
                self.created = true;
            }
            ContactEvent::Updated { name, email, phone } => {
                self.name = name.clone();
                self.email = email.clone();
                self.phone = phone.clone();
            }
            ContactEvent::Archived => {
                self.archived = true;
            }
            ContactEvent::CompanyLinked { company_id } => {
                self.company_id = Some(company_id.clone());
            }
            ContactEvent::CompanyUnlinked => {
                self.company_id = None;
            }
            ContactEvent::TagAdded { tag } => {
                self.tags.push(tag.clone());
            }
            ContactEvent::TagRemoved { tag } => {
                self.tags.retain(|t| t != tag);
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

    /// Helper: create a default contact and apply a Create command.
    fn created_contact() -> Contact {
        let c = Contact::default();
        let events = c
            .handle(ContactCommand::Create {
                name: "Alice".into(),
                email: "alice@example.com".into(),
                phone: "555-0100".into(),
            })
            .expect("create should succeed");
        events
            .into_iter()
            .fold(Contact::default(), |s, e| s.apply(&e))
    }

    #[test]
    fn create_contact() {
        let c = created_contact();
        assert_eq!(c.name, "Alice");
        assert_eq!(c.email, "alice@example.com");
        assert!(c.created);
    }

    #[test]
    fn reject_empty_name() {
        let c = Contact::default();
        let err = c
            .handle(ContactCommand::Create {
                name: "".into(),
                email: "a@b.com".into(),
                phone: "".into(),
            })
            .unwrap_err();
        assert!(matches!(err, ContactError::EmptyName));
    }

    #[test]
    fn reject_double_create() {
        let c = created_contact();
        let err = c
            .handle(ContactCommand::Create {
                name: "Bob".into(),
                email: "".into(),
                phone: "".into(),
            })
            .unwrap_err();
        assert!(matches!(err, ContactError::AlreadyExists));
    }

    #[test]
    fn update_contact() {
        let c = created_contact();
        let events = c
            .handle(ContactCommand::Update {
                name: "Alice B".into(),
                email: "alice.b@example.com".into(),
                phone: "555-0101".into(),
            })
            .expect("update should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert_eq!(c.name, "Alice B");
        assert_eq!(c.email, "alice.b@example.com");
    }

    #[test]
    fn archive_and_reject_double_archive() {
        let c = created_contact();
        let events = c
            .handle(ContactCommand::Archive)
            .expect("archive should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert!(c.archived);

        let err = c.handle(ContactCommand::Archive).unwrap_err();
        assert!(matches!(err, ContactError::AlreadyArchived));
    }

    #[test]
    fn reject_update_on_archived() {
        let mut c = created_contact();
        c.archived = true;
        let err = c
            .handle(ContactCommand::Update {
                name: "X".into(),
                email: "".into(),
                phone: "".into(),
            })
            .unwrap_err();
        assert!(matches!(err, ContactError::Archived));
    }

    #[test]
    fn link_and_unlink_company() {
        let c = created_contact();
        let events = c
            .handle(ContactCommand::LinkCompany {
                company_id: "comp-1".into(),
            })
            .expect("link should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert_eq!(c.company_id.as_deref(), Some("comp-1"));

        let events = c
            .handle(ContactCommand::Unlink)
            .expect("unlink should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert!(c.company_id.is_none());
    }

    #[test]
    fn add_and_remove_tags() {
        let c = created_contact();
        let events = c
            .handle(ContactCommand::AddTag { tag: "vip".into() })
            .expect("add tag should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert_eq!(c.tags, vec!["vip"]);

        // Duplicate tag rejected.
        let err = c
            .handle(ContactCommand::AddTag { tag: "vip".into() })
            .unwrap_err();
        assert!(matches!(err, ContactError::DuplicateTag(_)));

        let events = c
            .handle(ContactCommand::RemoveTag { tag: "vip".into() })
            .expect("remove tag should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert!(c.tags.is_empty());
    }
}
