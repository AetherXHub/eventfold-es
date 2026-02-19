//! Process managers (sagas) for cross-aggregate workflows.
//!
//! Process managers react to events from one aggregate and dispatch commands
//! to other aggregates, enabling long-running business processes.

use std::collections::{HashMap, HashSet};

use eventfold_es::{CommandContext, CommandEnvelope, ProcessManager};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DealTaskCreator
// ---------------------------------------------------------------------------

/// Automatically creates a follow-up task when a deal advances to the
/// Proposal stage.
///
/// Tracks deal titles from `Created` events so the auto-generated task
/// shows a human-readable name. Also tracks which deals have already had
/// tasks created to avoid duplicates on re-processing (idempotency guard).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DealTaskCreator {
    /// Deal IDs for which we have already emitted a task-creation command.
    created_for: HashSet<String>,
    /// Map of deal ID -> deal title, populated from `Created` events.
    titles: HashMap<String, String>,
}

impl ProcessManager for DealTaskCreator {
    const NAME: &'static str = "deal-task-creator";

    fn subscriptions(&self) -> &'static [&'static str] {
        &["deal"]
    }

    fn react(
        &mut self,
        _aggregate_type: &str,
        stream_id: &str,
        event: &eventfold::Event,
    ) -> Vec<CommandEnvelope> {
        // Track deal titles from Created events for human-readable task names.
        if event.event_type == "Created" {
            if let Some(title) = event.data.get("title").and_then(|v| v.as_str()) {
                self.titles.insert(stream_id.to_string(), title.to_string());
            }
            return vec![];
        }

        // Only react to StageAdvanced events targeting the Proposal stage.
        if event.event_type != "StageAdvanced" {
            return vec![];
        }

        let to_stage = event.data.get("to").and_then(|v| v.as_str()).unwrap_or("");

        if to_stage != "Proposal" {
            return vec![];
        }

        // Idempotency: skip if we already created a task for this deal.
        if !self.created_for.insert(stream_id.to_string()) {
            return vec![];
        }

        // Build a TaskCommand::Create targeting a new task instance.
        // The task ID is derived from the deal ID for determinism.
        let task_id = format!("auto-task-{stream_id}");
        let deal_name = self
            .titles
            .get(stream_id)
            .cloned()
            .unwrap_or_else(|| stream_id.to_string());
        let title = format!("Send proposal for {deal_name}");

        vec![CommandEnvelope {
            aggregate_type: "task".to_string(),
            instance_id: task_id,
            command: serde_json::json!({
                "type": "Create",
                "data": {
                    "title": title,
                    "description": "Auto-created when deal reached Proposal stage",
                    "deal_id": stream_id,
                    "contact_id": null,
                    "due": null
                }
            }),
            context: CommandContext::default()
                .with_actor("system:deal-task-creator")
                .with_correlation_id(format!("deal-proposal-{stream_id}")),
        }]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn created_event(title: &str) -> eventfold::Event {
        eventfold::Event {
            event_type: "Created".to_string(),
            data: serde_json::json!({
                "title": title,
                "company_id": "comp-1",
                "contact_id": "ct-1",
                "value": 10000
            }),
            ts: 1739980800,
            id: None,
            actor: None,
            meta: None,
        }
    }

    fn stage_advanced_event(to: &str) -> eventfold::Event {
        eventfold::Event {
            event_type: "StageAdvanced".to_string(),
            data: serde_json::json!({ "from": "Qualified", "to": to }),
            ts: 1739980800,
            id: None,
            actor: None,
            meta: None,
        }
    }

    #[test]
    fn creates_task_with_deal_title() {
        let mut pm = DealTaskCreator::default();

        // Feed the Created event so the PM learns the deal title.
        let created = pm.react("deal", "deal-42", &created_event("Enterprise License"));
        assert!(created.is_empty());

        let envelopes = pm.react("deal", "deal-42", &stage_advanced_event("Proposal"));
        assert_eq!(envelopes.len(), 1);
        let env = &envelopes[0];
        assert_eq!(env.aggregate_type, "task");
        assert_eq!(env.instance_id, "auto-task-deal-42");
        assert_eq!(
            env.command["data"]["title"],
            "Send proposal for Enterprise License"
        );
    }

    #[test]
    fn falls_back_to_deal_id_without_created_event() {
        let mut pm = DealTaskCreator::default();
        let envelopes = pm.react("deal", "deal-42", &stage_advanced_event("Proposal"));

        assert_eq!(envelopes.len(), 1);
        assert_eq!(
            envelopes[0].command["data"]["title"],
            "Send proposal for deal-42"
        );
    }

    #[test]
    fn ignores_non_proposal_stages() {
        let mut pm = DealTaskCreator::default();
        let envelopes = pm.react("deal", "deal-1", &stage_advanced_event("Qualified"));
        assert!(envelopes.is_empty());

        let envelopes = pm.react("deal", "deal-1", &stage_advanced_event("Negotiation"));
        assert!(envelopes.is_empty());
    }

    #[test]
    fn idempotent_no_duplicate_tasks() {
        let mut pm = DealTaskCreator::default();
        pm.react("deal", "deal-1", &created_event("Test Deal"));

        let first = pm.react("deal", "deal-1", &stage_advanced_event("Proposal"));
        assert_eq!(first.len(), 1);

        // React again for the same deal -- should produce nothing.
        let second = pm.react("deal", "deal-1", &stage_advanced_event("Proposal"));
        assert!(second.is_empty());
    }

    #[test]
    fn different_deals_get_separate_tasks() {
        let mut pm = DealTaskCreator::default();
        pm.react("deal", "deal-a", &created_event("Deal A"));
        pm.react("deal", "deal-b", &created_event("Deal B"));

        let a = pm.react("deal", "deal-a", &stage_advanced_event("Proposal"));
        let b = pm.react("deal", "deal-b", &stage_advanced_event("Proposal"));

        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_ne!(a[0].instance_id, b[0].instance_id);
    }
}
