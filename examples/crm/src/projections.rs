//! Read-model projections for the CRM dashboard.
//!
//! Projections subscribe to aggregate event streams and maintain denormalized
//! views optimized for querying. They are automatically caught up by the
//! [`AggregateStore`] whenever queried.

use std::collections::HashMap;

use eventfold_es::Projection;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// PipelineSummary
// ---------------------------------------------------------------------------

/// Summary of a single deal for the pipeline board.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DealSummary {
    /// Deal instance ID.
    pub deal_id: String,
    /// Deal title.
    pub title: String,
    /// Current stage label (e.g. "Prospect", "ClosedWon").
    pub stage: String,
    /// Monetary value in cents.
    pub value: u64,
}

/// Dashboard projection grouping deals by pipeline stage.
///
/// Subscribes to `deal` events and maintains a Kanban-style view with
/// value totals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineSummary {
    /// Deals grouped by stage name.
    pub by_stage: HashMap<String, Vec<DealSummary>>,
    /// Sum of all deal values.
    pub total_value: u64,
    /// Sum of values for won deals.
    pub won_value: u64,
    /// Sum of values for lost deals.
    pub lost_value: u64,
}

impl Projection for PipelineSummary {
    const NAME: &'static str = "pipeline-summary";

    fn subscriptions(&self) -> &'static [&'static str] {
        &["deal"]
    }

    fn apply(&mut self, _aggregate_type: &str, stream_id: &str, event: &eventfold::Event) {
        match event.event_type.as_str() {
            "Created" => {
                // Parse the event data to extract deal details.
                // The data field contains { title, company_id, contact_id, value }.
                let title = event
                    .data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let value = event
                    .data
                    .get("value")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let summary = DealSummary {
                    deal_id: stream_id.to_string(),
                    title,
                    stage: "Prospect".to_string(),
                    value,
                };
                self.total_value += value;
                self.by_stage
                    .entry("Prospect".to_string())
                    .or_default()
                    .push(summary);
            }
            "StageAdvanced" => {
                let from = event
                    .data
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let to = event.data.get("to").and_then(|v| v.as_str()).unwrap_or("");

                // Move the deal from the old stage column to the new one.
                if let Some(deals) = self.by_stage.get_mut(from)
                    && let Some(pos) = deals.iter().position(|d| d.deal_id == stream_id)
                {
                    let mut deal = deals.remove(pos);
                    deal.stage = to.to_string();
                    self.by_stage.entry(to.to_string()).or_default().push(deal);
                }
            }
            "ValueSet" => {
                let new_value = event
                    .data
                    .get("value")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                // Find the deal in whatever stage it's in and update.
                for deals in self.by_stage.values_mut() {
                    if let Some(deal) = deals.iter_mut().find(|d| d.deal_id == stream_id) {
                        self.total_value = self.total_value - deal.value + new_value;
                        deal.value = new_value;
                        return;
                    }
                }
            }
            "Won" => {
                // Move to ClosedWon, track won_value.
                let mut found = None;
                for (stage, deals) in self.by_stage.iter_mut() {
                    if let Some(pos) = deals.iter().position(|d| d.deal_id == stream_id) {
                        let mut deal = deals.remove(pos);
                        self.won_value += deal.value;
                        deal.stage = "ClosedWon".to_string();
                        // Stash the stage name to avoid borrow conflict.
                        let _ = stage;
                        found = Some(deal);
                        break;
                    }
                }
                if let Some(deal) = found {
                    self.by_stage
                        .entry("ClosedWon".to_string())
                        .or_default()
                        .push(deal);
                }
            }
            "Lost" => {
                let mut found = None;
                for deals in self.by_stage.values_mut() {
                    if let Some(pos) = deals.iter().position(|d| d.deal_id == stream_id) {
                        let mut deal = deals.remove(pos);
                        self.lost_value += deal.value;
                        deal.stage = "ClosedLost".to_string();
                        found = Some(deal);
                        break;
                    }
                }
                if let Some(deal) = found {
                    self.by_stage
                        .entry("ClosedLost".to_string())
                        .or_default()
                        .push(deal);
                }
            }
            _ => {} // Forward compatibility: ignore unknown events.
        }
    }
}

// ---------------------------------------------------------------------------
// ActivityFeed
// ---------------------------------------------------------------------------

/// A single entry in the activity timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEntry {
    /// Which aggregate type produced the event (e.g. "contact", "deal").
    pub aggregate_type: String,
    /// The aggregate instance ID.
    pub instance_id: String,
    /// The event type name (e.g. "Created", "StageAdvanced").
    pub event_type: String,
    /// Unix timestamp (seconds since epoch) from the event.
    pub ts: u64,
}

/// Recent-activity projection across all entity types.
///
/// Maintains a reverse-chronological feed capped at 200 entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActivityFeed {
    /// Activity entries, most recent first.
    pub entries: Vec<ActivityEntry>,
}

/// Maximum number of entries retained in the feed.
const FEED_CAP: usize = 200;

impl Projection for ActivityFeed {
    const NAME: &'static str = "activity-feed";

    fn subscriptions(&self) -> &'static [&'static str] {
        &["contact", "deal", "task", "note"]
    }

    fn apply(&mut self, aggregate_type: &str, stream_id: &str, event: &eventfold::Event) {
        let entry = ActivityEntry {
            aggregate_type: aggregate_type.to_string(),
            instance_id: stream_id.to_string(),
            event_type: event.event_type.clone(),
            ts: event.ts,
        };
        // Insert at front (most recent first).
        self.entries.insert(0, entry);
        // Enforce cap.
        self.entries.truncate(FEED_CAP);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build an eventfold::Event with the given type and optional data.
    fn make_event(event_type: &str, data: Option<serde_json::Value>) -> eventfold::Event {
        eventfold::Event {
            event_type: event_type.to_string(),
            data: data.unwrap_or(serde_json::Value::Null),
            ts: 1739980800, // 2025-02-19T12:00:00Z
            id: None,
            actor: None,
            meta: None,
        }
    }

    // -- PipelineSummary tests -----------------------------------------------

    #[test]
    fn pipeline_create_deals_at_different_stages() {
        let mut proj = PipelineSummary::default();

        // Create deal-1.
        proj.apply(
            "deal",
            "deal-1",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Alpha Deal",
                    "company_id": "c1",
                    "contact_id": "ct1",
                    "value": 10000
                })),
            ),
        );

        // Create deal-2.
        proj.apply(
            "deal",
            "deal-2",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Beta Deal",
                    "company_id": "c2",
                    "contact_id": "ct2",
                    "value": 20000
                })),
            ),
        );

        // Advance deal-1 to Qualified.
        proj.apply(
            "deal",
            "deal-1",
            &make_event(
                "StageAdvanced",
                Some(serde_json::json!({ "from": "Prospect", "to": "Qualified" })),
            ),
        );

        // Create deal-3 and advance to Proposal.
        proj.apply(
            "deal",
            "deal-3",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Gamma Deal",
                    "company_id": "c3",
                    "contact_id": "ct3",
                    "value": 30000
                })),
            ),
        );
        proj.apply(
            "deal",
            "deal-3",
            &make_event(
                "StageAdvanced",
                Some(serde_json::json!({ "from": "Prospect", "to": "Qualified" })),
            ),
        );
        proj.apply(
            "deal",
            "deal-3",
            &make_event(
                "StageAdvanced",
                Some(serde_json::json!({ "from": "Qualified", "to": "Proposal" })),
            ),
        );

        // Verify grouping.
        assert_eq!(proj.by_stage.get("Prospect").map(|v| v.len()), Some(1));
        assert_eq!(proj.by_stage.get("Qualified").map(|v| v.len()), Some(1));
        assert_eq!(proj.by_stage.get("Proposal").map(|v| v.len()), Some(1));
        assert_eq!(proj.total_value, 60000);
    }

    #[test]
    fn pipeline_win_and_lose() {
        let mut proj = PipelineSummary::default();

        proj.apply(
            "deal",
            "d1",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Win",
                    "company_id": "",
                    "contact_id": "",
                    "value": 5000
                })),
            ),
        );
        proj.apply(
            "deal",
            "d2",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Lose",
                    "company_id": "",
                    "contact_id": "",
                    "value": 3000
                })),
            ),
        );

        proj.apply("deal", "d1", &make_event("Won", None));
        proj.apply("deal", "d2", &make_event("Lost", None));

        assert_eq!(proj.won_value, 5000);
        assert_eq!(proj.lost_value, 3000);
        assert_eq!(proj.by_stage.get("ClosedWon").map(|v| v.len()), Some(1));
        assert_eq!(proj.by_stage.get("ClosedLost").map(|v| v.len()), Some(1));
        assert!(
            proj.by_stage
                .get("Prospect")
                .map(|v| v.is_empty())
                .unwrap_or(true)
        );
    }

    #[test]
    fn pipeline_value_set() {
        let mut proj = PipelineSummary::default();

        proj.apply(
            "deal",
            "d1",
            &make_event(
                "Created",
                Some(serde_json::json!({
                    "title": "Test",
                    "company_id": "",
                    "contact_id": "",
                    "value": 1000
                })),
            ),
        );
        assert_eq!(proj.total_value, 1000);

        proj.apply(
            "deal",
            "d1",
            &make_event("ValueSet", Some(serde_json::json!({ "value": 5000 }))),
        );
        assert_eq!(proj.total_value, 5000);
    }

    // -- ActivityFeed tests --------------------------------------------------

    #[test]
    fn activity_feed_records_events_most_recent_first() {
        let mut feed = ActivityFeed::default();

        feed.apply("contact", "ct-1", &make_event("Created", None));
        feed.apply("deal", "d-1", &make_event("Created", None));
        feed.apply("task", "t-1", &make_event("Created", None));

        assert_eq!(feed.entries.len(), 3);
        // Most recent is first.
        assert_eq!(feed.entries[0].aggregate_type, "task");
        assert_eq!(feed.entries[2].aggregate_type, "contact");
    }

    #[test]
    fn activity_feed_caps_at_200() {
        let mut feed = ActivityFeed::default();
        for i in 0..250 {
            feed.apply("note", &format!("n-{i}"), &make_event("Created", None));
        }
        assert_eq!(feed.entries.len(), FEED_CAP);
        // The most recent entry should be the last one inserted.
        assert_eq!(feed.entries[0].instance_id, "n-249");
    }
}
