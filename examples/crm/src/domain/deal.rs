//! Deal aggregate -- sales opportunities in the pipeline.
//!
//! A deal progresses through stages: Prospect -> Qualified -> Proposal ->
//! Negotiation. From Negotiation it can only be Won or Lost (never advanced
//! further). Winning or losing closes the deal permanently.

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Sales pipeline stages.
///
/// Stages are ordered; `Advance` moves a deal to the next stage in sequence
/// up to `Negotiation`. The final two stages (`ClosedWon`, `ClosedLost`) are
/// set only by the `Win` / `Lose` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DealStage {
    #[default]
    Prospect,
    Qualified,
    Proposal,
    Negotiation,
    ClosedWon,
    ClosedLost,
}

impl DealStage {
    /// Return the display label for this stage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Prospect => "Prospect",
            Self::Qualified => "Qualified",
            Self::Proposal => "Proposal",
            Self::Negotiation => "Negotiation",
            Self::ClosedWon => "ClosedWon",
            Self::ClosedLost => "ClosedLost",
        }
    }

    /// Attempt to advance to the next stage. Returns `None` if already at
    /// `Negotiation` or beyond (use `Win`/`Lose` instead).
    fn next(self) -> Option<Self> {
        match self {
            Self::Prospect => Some(Self::Qualified),
            Self::Qualified => Some(Self::Proposal),
            Self::Proposal => Some(Self::Negotiation),
            Self::Negotiation | Self::ClosedWon | Self::ClosedLost => None,
        }
    }
}

/// A CRM deal (sales opportunity).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Deal {
    /// Deal title / description.
    pub title: String,
    /// The company this deal belongs to.
    pub company_id: String,
    /// The primary contact for this deal.
    pub contact_id: String,
    /// Current pipeline stage.
    pub stage: DealStage,
    /// Monetary value in cents (avoids floating-point).
    pub value: u64,
    /// Whether the deal is closed (won or lost).
    pub closed: bool,
    /// Whether the deal has been created.
    pub created: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Deal`] aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DealCommand {
    /// Create a new deal in the Prospect stage.
    Create {
        title: String,
        company_id: String,
        contact_id: String,
        value: u64,
    },
    /// Advance the deal to the next pipeline stage.
    Advance,
    /// Set the monetary value.
    SetValue { value: u64 },
    /// Close the deal as won.
    Win,
    /// Close the deal as lost.
    Lose,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Deal`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DealEvent {
    /// A new deal was created.
    Created {
        title: String,
        company_id: String,
        contact_id: String,
        value: u64,
    },
    /// The deal advanced to a new stage.
    StageAdvanced { from: DealStage, to: DealStage },
    /// The monetary value was updated.
    ValueSet { value: u64 },
    /// The deal was closed as won.
    Won,
    /// The deal was closed as lost.
    Lost,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur when handling a [`DealCommand`].
#[derive(Debug, thiserror::Error)]
pub enum DealError {
    /// The deal title must not be empty.
    #[error("deal title must not be empty")]
    EmptyTitle,
    /// Attempted to create a deal that already exists.
    #[error("deal already exists")]
    AlreadyExists,
    /// Attempted to advance a deal that is already closed.
    #[error("deal is closed")]
    AlreadyClosed,
    /// Attempted to advance past the Negotiation stage.
    #[error("cannot advance past Negotiation; use Win or Lose")]
    CannotAdvance,
    /// Attempted to modify a deal that has not been created.
    #[error("deal does not exist")]
    NotCreated,
}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Deal {
    const AGGREGATE_TYPE: &'static str = "deal";
    type Command = DealCommand;
    type DomainEvent = DealEvent;
    type Error = DealError;

    fn handle(&self, cmd: DealCommand) -> Result<Vec<DealEvent>, DealError> {
        match cmd {
            DealCommand::Create {
                title,
                company_id,
                contact_id,
                value,
            } => {
                if title.trim().is_empty() {
                    return Err(DealError::EmptyTitle);
                }
                if self.created {
                    return Err(DealError::AlreadyExists);
                }
                Ok(vec![DealEvent::Created {
                    title,
                    company_id,
                    contact_id,
                    value,
                }])
            }
            DealCommand::Advance => {
                if !self.created {
                    return Err(DealError::NotCreated);
                }
                if self.closed {
                    return Err(DealError::AlreadyClosed);
                }
                let to = self.stage.next().ok_or(DealError::CannotAdvance)?;
                Ok(vec![DealEvent::StageAdvanced {
                    from: self.stage,
                    to,
                }])
            }
            DealCommand::SetValue { value } => {
                if !self.created {
                    return Err(DealError::NotCreated);
                }
                if self.closed {
                    return Err(DealError::AlreadyClosed);
                }
                Ok(vec![DealEvent::ValueSet { value }])
            }
            DealCommand::Win => {
                if !self.created {
                    return Err(DealError::NotCreated);
                }
                if self.closed {
                    return Err(DealError::AlreadyClosed);
                }
                Ok(vec![DealEvent::Won])
            }
            DealCommand::Lose => {
                if !self.created {
                    return Err(DealError::NotCreated);
                }
                if self.closed {
                    return Err(DealError::AlreadyClosed);
                }
                Ok(vec![DealEvent::Lost])
            }
        }
    }

    fn apply(mut self, event: &DealEvent) -> Self {
        match event {
            DealEvent::Created {
                title,
                company_id,
                contact_id,
                value,
            } => {
                self.title = title.clone();
                self.company_id = company_id.clone();
                self.contact_id = contact_id.clone();
                self.value = *value;
                self.stage = DealStage::Prospect;
                self.created = true;
            }
            DealEvent::StageAdvanced { to, .. } => {
                self.stage = *to;
            }
            DealEvent::ValueSet { value } => {
                self.value = *value;
            }
            DealEvent::Won => {
                self.stage = DealStage::ClosedWon;
                self.closed = true;
            }
            DealEvent::Lost => {
                self.stage = DealStage::ClosedLost;
                self.closed = true;
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

    fn created_deal() -> Deal {
        let d = Deal::default();
        let events = d
            .handle(DealCommand::Create {
                title: "Big Sale".into(),
                company_id: "comp-1".into(),
                contact_id: "contact-1".into(),
                value: 50_000_00,
            })
            .expect("create should succeed");
        events.into_iter().fold(Deal::default(), |s, e| s.apply(&e))
    }

    #[test]
    fn create_deal() {
        let d = created_deal();
        assert_eq!(d.title, "Big Sale");
        assert_eq!(d.stage, DealStage::Prospect);
        assert!(!d.closed);
    }

    #[test]
    fn advance_through_stages() {
        let mut d = created_deal();
        for expected in [
            DealStage::Qualified,
            DealStage::Proposal,
            DealStage::Negotiation,
        ] {
            let events = d.handle(DealCommand::Advance).expect("advance ok");
            d = events.into_iter().fold(d, |s, e| s.apply(&e));
            assert_eq!(d.stage, expected);
        }
        // Cannot advance past Negotiation.
        let err = d.handle(DealCommand::Advance).unwrap_err();
        assert!(matches!(err, DealError::CannotAdvance));
    }

    #[test]
    fn win_deal() {
        let d = created_deal();
        let events = d.handle(DealCommand::Win).expect("win ok");
        let d = events.into_iter().fold(d, |s, e| s.apply(&e));
        assert_eq!(d.stage, DealStage::ClosedWon);
        assert!(d.closed);

        // Cannot win again.
        let err = d.handle(DealCommand::Win).unwrap_err();
        assert!(matches!(err, DealError::AlreadyClosed));
    }

    #[test]
    fn lose_deal() {
        let d = created_deal();
        let events = d.handle(DealCommand::Lose).expect("lose ok");
        let d = events.into_iter().fold(d, |s, e| s.apply(&e));
        assert_eq!(d.stage, DealStage::ClosedLost);
        assert!(d.closed);
    }

    #[test]
    fn reject_empty_title() {
        let d = Deal::default();
        let err = d
            .handle(DealCommand::Create {
                title: "".into(),
                company_id: "".into(),
                contact_id: "".into(),
                value: 0,
            })
            .unwrap_err();
        assert!(matches!(err, DealError::EmptyTitle));
    }

    #[test]
    fn set_value() {
        let d = created_deal();
        let events = d
            .handle(DealCommand::SetValue { value: 100_000_00 })
            .expect("set value ok");
        let d = events.into_iter().fold(d, |s, e| s.apply(&e));
        assert_eq!(d.value, 100_000_00);
    }

    #[test]
    fn reject_advance_closed_deal() {
        let d = created_deal();
        let events = d.handle(DealCommand::Win).expect("win ok");
        let d = events.into_iter().fold(d, |s, e| s.apply(&e));

        let err = d.handle(DealCommand::Advance).unwrap_err();
        assert!(matches!(err, DealError::AlreadyClosed));
    }
}
