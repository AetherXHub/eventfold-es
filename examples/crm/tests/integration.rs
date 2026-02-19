//! Integration tests for the EventFold CRM demo.
//!
//! These tests exercise full saga roundtrips, projection correctness, and
//! process manager idempotency using a temporary store directory.

use std::time::Duration;

use eventfold_es::{AggregateStore, CommandBus, CommandContext};

// Re-use domain types from the CRM crate.
use app_lib::domain::{
    Company, CompanyCommand, Contact, ContactCommand, Deal, DealCommand, Note, NoteCommand, Task,
};
use app_lib::projections::{ActivityFeed, PipelineSummary};
use app_lib::workflows::DealTaskCreator;

/// Build a fully configured store in a temporary directory.
async fn test_store(dir: &std::path::Path) -> (AggregateStore, CommandBus) {
    let store = AggregateStore::builder(dir)
        .projection::<PipelineSummary>()
        .projection::<ActivityFeed>()
        .process_manager::<DealTaskCreator>()
        .aggregate_type::<Task>()
        .idle_timeout(Duration::from_secs(60))
        .open()
        .await
        .expect("failed to open store");

    let mut bus = CommandBus::new(store.clone());
    bus.register::<Contact>();
    bus.register::<Company>();
    bus.register::<Deal>();
    bus.register::<Task>();
    bus.register::<Note>();

    (store, bus)
}

fn ctx() -> CommandContext {
    CommandContext::default().with_actor("test")
}

/// Full saga: create company, contact, deal; advance deal to Proposal;
/// verify DealTaskCreator auto-created a follow-up task.
#[tokio::test]
async fn full_saga_roundtrip() {
    let tmp = tempfile::tempdir().expect("failed to create tmpdir");
    let (store, bus) = test_store(tmp.path()).await;

    // Create a company.
    bus.dispatch(
        "comp-1",
        CompanyCommand::Create {
            name: "Acme Corp".into(),
            industry: "Tech".into(),
            website: None,
        },
        ctx(),
    )
    .await
    .expect("create company");

    // Create a contact linked to the company.
    bus.dispatch(
        "ct-1",
        ContactCommand::Create {
            name: "Alice".into(),
            email: "alice@acme.example".into(),
            phone: "555-0100".into(),
        },
        ctx(),
    )
    .await
    .expect("create contact");

    bus.dispatch(
        "ct-1",
        ContactCommand::LinkCompany {
            company_id: "comp-1".into(),
        },
        ctx(),
    )
    .await
    .expect("link contact to company");

    // Create a deal.
    bus.dispatch(
        "deal-1",
        DealCommand::Create {
            title: "Enterprise License".into(),
            company_id: "comp-1".into(),
            contact_id: "ct-1".into(),
            value: 250_000_00,
        },
        ctx(),
    )
    .await
    .expect("create deal");

    // Advance deal: Prospect -> Qualified -> Proposal.
    bus.dispatch("deal-1", DealCommand::Advance, ctx())
        .await
        .expect("advance to Qualified");

    bus.dispatch("deal-1", DealCommand::Advance, ctx())
        .await
        .expect("advance to Proposal");

    // Run process managers -- should auto-create a task.
    let report = store
        .run_process_managers()
        .await
        .expect("run process managers");
    assert_eq!(report.dispatched, 1, "should dispatch exactly one task");

    // Verify the auto-created task exists.
    let task_handle = store
        .get::<Task>("auto-task-deal-1")
        .await
        .expect("get auto task");
    let task_state = task_handle.state().await.expect("task state");
    assert!(task_state.created);
    assert!(task_state.title.contains("Enterprise License"));
    assert_eq!(task_state.deal_id.as_deref(), Some("deal-1"));
}

/// PipelineSummary correctly groups deals by stage and tracks values.
#[tokio::test]
async fn pipeline_summary_projection() {
    let tmp = tempfile::tempdir().expect("failed to create tmpdir");
    let (store, bus) = test_store(tmp.path()).await;

    // Create 3 deals at different values.
    for (id, title, value) in [
        ("d1", "Deal A", 10_000_00u64),
        ("d2", "Deal B", 20_000_00),
        ("d3", "Deal C", 30_000_00),
    ] {
        bus.dispatch(
            id,
            DealCommand::Create {
                title: title.into(),
                company_id: "comp".into(),
                contact_id: "ct".into(),
                value,
            },
            ctx(),
        )
        .await
        .expect("create deal");
    }

    // Advance d1 to Qualified, d2 to Proposal.
    bus.dispatch("d1", DealCommand::Advance, ctx())
        .await
        .expect("advance d1");
    bus.dispatch("d2", DealCommand::Advance, ctx())
        .await
        .expect("advance d2 step 1");
    bus.dispatch("d2", DealCommand::Advance, ctx())
        .await
        .expect("advance d2 step 2");

    // Win d1.
    bus.dispatch("d1", DealCommand::Win, ctx())
        .await
        .expect("win d1");

    // Query the projection.
    let pipeline = store
        .projection::<PipelineSummary>()
        .expect("pipeline projection");

    assert_eq!(pipeline.total_value, 60_000_00);
    assert_eq!(pipeline.won_value, 10_000_00);
    assert_eq!(pipeline.lost_value, 0);

    // d3 still in Prospect, d2 in Proposal, d1 in ClosedWon.
    assert_eq!(
        pipeline
            .by_stage
            .get("Prospect")
            .map(|v| v.len())
            .unwrap_or(0),
        1
    );
    assert_eq!(
        pipeline
            .by_stage
            .get("Proposal")
            .map(|v| v.len())
            .unwrap_or(0),
        1
    );
    assert_eq!(
        pipeline
            .by_stage
            .get("ClosedWon")
            .map(|v| v.len())
            .unwrap_or(0),
        1
    );
}

/// Running process managers twice does not create duplicate tasks.
#[tokio::test]
async fn process_manager_idempotency() {
    let tmp = tempfile::tempdir().expect("failed to create tmpdir");
    let (store, bus) = test_store(tmp.path()).await;

    // Create and advance a deal to Proposal.
    bus.dispatch(
        "deal-x",
        DealCommand::Create {
            title: "Idempotency Test".into(),
            company_id: "c".into(),
            contact_id: "ct".into(),
            value: 1000,
        },
        ctx(),
    )
    .await
    .expect("create deal");

    bus.dispatch("deal-x", DealCommand::Advance, ctx())
        .await
        .expect("advance to Qualified");
    bus.dispatch("deal-x", DealCommand::Advance, ctx())
        .await
        .expect("advance to Proposal");

    // First run: should dispatch 1 task.
    let r1 = store.run_process_managers().await.expect("first PM run");
    assert_eq!(r1.dispatched, 1);

    // Second run: should dispatch 0 (idempotent).
    let r2 = store.run_process_managers().await.expect("second PM run");
    assert_eq!(r2.dispatched, 0);

    // Verify only one task exists.
    let tasks = store.list::<Task>().await.expect("list tasks");
    assert_eq!(tasks.len(), 1);
}

/// ActivityFeed records events across multiple entity types.
#[tokio::test]
async fn activity_feed_projection() {
    let tmp = tempfile::tempdir().expect("failed to create tmpdir");
    let (store, bus) = test_store(tmp.path()).await;

    bus.dispatch(
        "ct-1",
        ContactCommand::Create {
            name: "Bob".into(),
            email: "".into(),
            phone: "".into(),
        },
        ctx(),
    )
    .await
    .expect("create contact");

    bus.dispatch(
        "d-1",
        DealCommand::Create {
            title: "Test".into(),
            company_id: "".into(),
            contact_id: "".into(),
            value: 0,
        },
        ctx(),
    )
    .await
    .expect("create deal");

    bus.dispatch(
        "n-1",
        NoteCommand::Create {
            body: "Hello".into(),
            contact_id: None,
            deal_id: None,
            company_id: None,
            task_id: None,
        },
        ctx(),
    )
    .await
    .expect("create note");

    let feed = store.projection::<ActivityFeed>().expect("activity feed");

    // Should have at least 3 entries (one per create).
    assert!(feed.entries.len() >= 3);
    // Most recent first: note was last.
    assert_eq!(feed.entries[0].aggregate_type, "note");
}
