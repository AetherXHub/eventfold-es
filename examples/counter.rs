//! Self-contained example demonstrating aggregates, projections, and the
//! builder-based `AggregateStoreBuilder` API backed by an `eventfold-db`
//! gRPC server.
//!
//! Run with: `cargo run --example counter`
//!
//! **Requires** a running `eventfold-db` server on `http://127.0.0.1:2113`.

use eventfold_es::{Aggregate, AggregateStoreBuilder, CommandContext, Projection, StoredEvent};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Counter aggregate
// ---------------------------------------------------------------------------

/// A simple counter that can be incremented, decremented, or reset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Counter {
    value: i64,
}

/// Commands accepted by the [`Counter`] aggregate.
#[derive(Clone)]
enum CounterCommand {
    Increment,
    Decrement,
    Reset,
}

/// Domain events produced by the [`Counter`] aggregate.
///
/// Uses adjacently tagged serde serialization -- the required format for
/// all `eventfold-es` domain events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
enum CounterEvent {
    Incremented,
    Decremented,
    WasReset { previous: i64 },
}

/// Errors that can occur when handling a [`CounterCommand`].
#[derive(Debug, thiserror::Error)]
enum CounterError {
    #[error("counter is already zero, cannot decrement")]
    AlreadyZero,
    #[error("counter is already zero, nothing to reset")]
    NothingToReset,
}

impl Aggregate for Counter {
    const AGGREGATE_TYPE: &'static str = "counter";
    type Command = CounterCommand;
    type DomainEvent = CounterEvent;
    type Error = CounterError;

    fn handle(&self, cmd: CounterCommand) -> Result<Vec<CounterEvent>, CounterError> {
        match cmd {
            CounterCommand::Increment => Ok(vec![CounterEvent::Incremented]),
            CounterCommand::Decrement => {
                if self.value <= 0 {
                    return Err(CounterError::AlreadyZero);
                }
                Ok(vec![CounterEvent::Decremented])
            }
            CounterCommand::Reset => {
                if self.value == 0 {
                    return Err(CounterError::NothingToReset);
                }
                Ok(vec![CounterEvent::WasReset {
                    previous: self.value,
                }])
            }
        }
    }

    fn apply(mut self, event: &CounterEvent) -> Self {
        match event {
            CounterEvent::Incremented => self.value += 1,
            CounterEvent::Decremented => self.value -= 1,
            CounterEvent::WasReset { .. } => self.value = 0,
        }
        self
    }
}

// ---------------------------------------------------------------------------
// TotalCounter projection (cross-instance read model)
// ---------------------------------------------------------------------------

/// Sums the value across all counter instances.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TotalCounter {
    total_increments: u64,
    total_decrements: u64,
    total_resets: u64,
}

impl Projection for TotalCounter {
    const NAME: &'static str = "total-counter";

    fn apply(&mut self, event: &StoredEvent) {
        // Only react to "counter" aggregate events.
        if event.aggregate_type != "counter" {
            return;
        }
        match event.event_type.as_str() {
            "Incremented" => self.total_increments += 1,
            "Decremented" => self.total_decrements += 1,
            "WasReset" => self.total_resets += 1,
            _ => {} // Forward compatibility: ignore unknown events.
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use a temporary directory for local caches (snapshots, checkpoints).
    let tmp = tempfile::tempdir()?;

    // Build the store via the builder API.
    let store = AggregateStoreBuilder::new()
        .endpoint("http://127.0.0.1:2113")
        .base_dir(tmp.path())
        .projection::<TotalCounter>()
        .open()
        .await?;

    let ctx = CommandContext::default().with_actor("example-runner");

    // Create two counter instances and send commands.
    let alpha = store.get::<Counter>("alpha").await?;
    alpha
        .execute(CounterCommand::Increment, ctx.clone())
        .await?;
    alpha
        .execute(CounterCommand::Increment, ctx.clone())
        .await?;
    alpha
        .execute(CounterCommand::Increment, ctx.clone())
        .await?;

    let beta = store.get::<Counter>("beta").await?;
    beta.execute(CounterCommand::Increment, ctx.clone()).await?;
    beta.execute(CounterCommand::Decrement, ctx.clone()).await?;
    beta.execute(CounterCommand::Increment, ctx.clone()).await?;

    // Reset alpha.
    alpha.execute(CounterCommand::Reset, ctx).await?;

    // Query individual aggregate state.
    let alpha_state = alpha.state().await?;
    let beta_state = beta.state().await?;

    println!("alpha = {}", alpha_state.value);
    println!("beta  = {}", beta_state.value);

    // Query the projection (catches up automatically).
    let totals = store.projection::<TotalCounter>().await?;
    println!(
        "totals: increments={}, decrements={}, resets={}",
        totals.total_increments, totals.total_decrements, totals.total_resets
    );

    // Verify expected values.
    assert_eq!(alpha_state.value, 0, "alpha should be reset to 0");
    assert_eq!(beta_state.value, 1, "beta should be 1 (inc, dec, inc)");
    assert_eq!(totals.total_increments, 5);
    assert_eq!(totals.total_decrements, 1);
    assert_eq!(totals.total_resets, 1);

    println!("all assertions passed");

    Ok(())
}
