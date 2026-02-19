//! Company aggregate -- organizations in the CRM.
//!
//! Companies are simple entities with a name, industry, and optional website.
//! They can be archived but not un-archived.

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A CRM company (organization).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Company {
    /// Company display name.
    pub name: String,
    /// Industry sector.
    pub industry: String,
    /// Optional website URL.
    pub website: Option<String>,
    /// Whether this company has been archived.
    pub archived: bool,
    /// Whether the company has been created.
    pub created: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Company`] aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CompanyCommand {
    /// Create a new company.
    Create {
        name: String,
        industry: String,
        website: Option<String>,
    },
    /// Update mutable fields.
    Update {
        name: String,
        industry: String,
        website: Option<String>,
    },
    /// Soft-delete the company.
    Archive,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Company`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CompanyEvent {
    /// A new company was created.
    Created {
        name: String,
        industry: String,
        website: Option<String>,
    },
    /// Company fields were updated.
    Updated {
        name: String,
        industry: String,
        website: Option<String>,
    },
    /// The company was archived.
    Archived,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when handling a [`CompanyCommand`].
#[derive(Debug, thiserror::Error)]
pub enum CompanyError {
    /// The company name must not be empty.
    #[error("company name must not be empty")]
    EmptyName,
    /// Attempted to create a company that already exists.
    #[error("company already exists")]
    AlreadyExists,
    /// Attempted to modify an archived company.
    #[error("company is archived")]
    Archived,
    /// Attempted to archive a company that is already archived.
    #[error("company is already archived")]
    AlreadyArchived,
    /// Attempted to modify a company that has not been created.
    #[error("company does not exist")]
    NotCreated,
}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Company {
    const AGGREGATE_TYPE: &'static str = "company";
    type Command = CompanyCommand;
    type DomainEvent = CompanyEvent;
    type Error = CompanyError;

    fn handle(&self, cmd: CompanyCommand) -> Result<Vec<CompanyEvent>, CompanyError> {
        match cmd {
            CompanyCommand::Create {
                name,
                industry,
                website,
            } => {
                if name.trim().is_empty() {
                    return Err(CompanyError::EmptyName);
                }
                if self.created {
                    return Err(CompanyError::AlreadyExists);
                }
                Ok(vec![CompanyEvent::Created {
                    name,
                    industry,
                    website,
                }])
            }
            CompanyCommand::Update {
                name,
                industry,
                website,
            } => {
                if !self.created {
                    return Err(CompanyError::NotCreated);
                }
                if self.archived {
                    return Err(CompanyError::Archived);
                }
                if name.trim().is_empty() {
                    return Err(CompanyError::EmptyName);
                }
                Ok(vec![CompanyEvent::Updated {
                    name,
                    industry,
                    website,
                }])
            }
            CompanyCommand::Archive => {
                if !self.created {
                    return Err(CompanyError::NotCreated);
                }
                if self.archived {
                    return Err(CompanyError::AlreadyArchived);
                }
                Ok(vec![CompanyEvent::Archived])
            }
        }
    }

    fn apply(mut self, event: &CompanyEvent) -> Self {
        match event {
            CompanyEvent::Created {
                name,
                industry,
                website,
            } => {
                self.name = name.clone();
                self.industry = industry.clone();
                self.website = website.clone();
                self.created = true;
            }
            CompanyEvent::Updated {
                name,
                industry,
                website,
            } => {
                self.name = name.clone();
                self.industry = industry.clone();
                self.website = website.clone();
            }
            CompanyEvent::Archived => {
                self.archived = true;
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

    fn created_company() -> Company {
        let c = Company::default();
        let events = c
            .handle(CompanyCommand::Create {
                name: "Acme Corp".into(),
                industry: "Tech".into(),
                website: Some("https://acme.example".into()),
            })
            .expect("create should succeed");
        events
            .into_iter()
            .fold(Company::default(), |s, e| s.apply(&e))
    }

    #[test]
    fn create_company() {
        let c = created_company();
        assert_eq!(c.name, "Acme Corp");
        assert_eq!(c.industry, "Tech");
        assert!(c.created);
    }

    #[test]
    fn reject_empty_name() {
        let c = Company::default();
        let err = c
            .handle(CompanyCommand::Create {
                name: "  ".into(),
                industry: "".into(),
                website: None,
            })
            .unwrap_err();
        assert!(matches!(err, CompanyError::EmptyName));
    }

    #[test]
    fn reject_double_create() {
        let c = created_company();
        let err = c
            .handle(CompanyCommand::Create {
                name: "Other".into(),
                industry: "".into(),
                website: None,
            })
            .unwrap_err();
        assert!(matches!(err, CompanyError::AlreadyExists));
    }

    #[test]
    fn update_company() {
        let c = created_company();
        let events = c
            .handle(CompanyCommand::Update {
                name: "Acme Inc".into(),
                industry: "SaaS".into(),
                website: None,
            })
            .expect("update should succeed");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert_eq!(c.name, "Acme Inc");
        assert!(c.website.is_none());
    }

    #[test]
    fn archive_and_reject_double_archive() {
        let c = created_company();
        let events = c.handle(CompanyCommand::Archive).expect("archive ok");
        let c = events.into_iter().fold(c, |s, e| s.apply(&e));
        assert!(c.archived);

        let err = c.handle(CompanyCommand::Archive).unwrap_err();
        assert!(matches!(err, CompanyError::AlreadyArchived));
    }

    #[test]
    fn reject_update_on_archived() {
        let mut c = created_company();
        c.archived = true;
        let err = c
            .handle(CompanyCommand::Update {
                name: "X".into(),
                industry: "".into(),
                website: None,
            })
            .unwrap_err();
        assert!(matches!(err, CompanyError::Archived));
    }
}
