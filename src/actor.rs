//! Actor loop that owns an aggregate and processes commands.
//!
//! The actor runs on a blocking thread and sequentially processes messages
//! from an `mpsc` channel. It exclusively owns the `EventWriter` (and
//! therefore the flock), the `View<A>` for maintaining aggregate state,
//! and the `EventReader` for refreshing the view before each command.
//!
//! Public API: [`AggregateHandle`] (cloneable async handle) and
//! [`spawn_actor`] (factory that opens the log and starts the actor thread).

use std::io;
use std::path::Path;

use eventfold::{EventReader, EventWriter, View};
use tokio::sync::{mpsc, oneshot};

use crate::aggregate::{Aggregate, reducer, to_eventfold_event};
use crate::command::CommandContext;
use crate::error::{ExecuteError, StateError};

/// Maximum number of optimistic concurrency retries before giving up.
///
/// Currently unused because the actor exclusively owns the writer, so
/// conflicts cannot occur. Retained for future `append_if` support.
#[allow(dead_code)]
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Result type sent back through the `Execute` reply channel.
type ExecuteResult<A> =
    Result<Vec<<A as Aggregate>::DomainEvent>, ExecuteError<<A as Aggregate>::Error>>;

/// Messages sent from `AggregateHandle` to the actor loop.
///
/// Each variant carries a `oneshot::Sender` for the actor to reply on
/// once the operation completes.
pub(crate) enum ActorMessage<A: Aggregate> {
    /// Execute a command against the aggregate.
    Execute {
        /// The domain command to execute.
        cmd: A::Command,
        /// Cross-cutting metadata (actor identity, correlation ID, etc.).
        ctx: CommandContext,
        /// Channel to send back the produced domain events or an error.
        reply: oneshot::Sender<ExecuteResult<A>>,
    },

    /// Retrieve the current aggregate state.
    GetState {
        /// Channel to send back a clone of the current state or an error.
        reply: oneshot::Sender<Result<A, StateError>>,
    },

    /// Gracefully shut down the actor loop.
    #[allow(dead_code)]
    Shutdown,
}

/// Runs the aggregate actor loop on a blocking thread.
///
/// Owns the `EventWriter` and aggregate state `View`. Receives messages
/// from `AggregateHandle` via the mpsc channel and processes them sequentially.
/// The loop exits when the channel closes (all senders dropped) or a
/// `Shutdown` message is received. On exit the `EventWriter` is dropped,
/// releasing the flock.
///
/// # Arguments
///
/// * `writer` - Exclusive writer for the aggregate's event stream.
/// * `view` - Derived view that holds the current aggregate state.
/// * `reader` - Reader used to refresh the view before each operation.
/// * `rx` - Receiving end of the mpsc channel carrying `ActorMessage`s.
pub(crate) fn run_actor<A: Aggregate>(
    mut writer: EventWriter,
    mut view: View<A>,
    reader: EventReader,
    mut rx: mpsc::Receiver<ActorMessage<A>>,
) {
    // `blocking_recv` blocks the current thread until a message arrives
    // or the channel closes (all senders dropped). This is appropriate
    // because this function runs inside `spawn_blocking`.
    while let Some(msg) = rx.blocking_recv() {
        match msg {
            ActorMessage::Execute { cmd, ctx, reply } => {
                let result = execute_command::<A>(&mut writer, &mut view, &reader, cmd, &ctx);
                // If the receiver was dropped, the caller no longer cares
                // about the result. Silently discard it.
                let _ = reply.send(result);
            }

            ActorMessage::GetState { reply } => {
                let result = get_state::<A>(&mut view, &reader);
                let _ = reply.send(result);
            }

            ActorMessage::Shutdown => break,
        }
    }
    // Loop exited: either `Shutdown` received or all senders dropped.
    // `writer` is dropped here, releasing the flock on the event log.
}

/// Execute a single command: refresh state, handle the command, persist events.
///
/// Factored out of the match arm for clarity and to keep the actor loop concise.
fn execute_command<A: Aggregate>(
    writer: &mut EventWriter,
    view: &mut View<A>,
    reader: &EventReader,
    cmd: A::Command,
    ctx: &CommandContext,
) -> Result<Vec<A::DomainEvent>, ExecuteError<A::Error>> {
    // 1. Refresh the view to incorporate any events not yet folded.
    //    Although we are the sole writer, a prior iteration may have
    //    appended events that the view hasn't consumed yet.
    view.refresh(reader).map_err(ExecuteError::Io)?;

    // 2. Decide: run the command handler against current state.
    let state = view.state().clone();
    let domain_events = state.handle(cmd).map_err(ExecuteError::Domain)?;

    // 3. No-op commands produce no events.
    if domain_events.is_empty() {
        return Ok(domain_events);
    }

    // 4. Convert each domain event to an `eventfold::Event` and append.
    for de in &domain_events {
        let ef_event = to_eventfold_event::<A>(de, ctx)
            .map_err(|e| ExecuteError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
        writer.append(&ef_event).map_err(ExecuteError::Io)?;
    }

    Ok(domain_events)
}

/// Refresh the view and return a clone of the current aggregate state.
fn get_state<A: Aggregate>(view: &mut View<A>, reader: &EventReader) -> Result<A, StateError> {
    view.refresh(reader).map_err(StateError::Io)?;
    Ok(view.state().clone())
}

/// Async handle to a running aggregate actor.
///
/// Lightweight, cloneable, and `Send + Sync`. Communicates with the
/// actor thread over a bounded channel.
///
/// # Type Parameters
///
/// * `A` - The [`Aggregate`] type this handle controls.
#[derive(Debug)]
pub struct AggregateHandle<A: Aggregate> {
    sender: mpsc::Sender<ActorMessage<A>>,
    reader: EventReader,
}

// Manual `Clone` because `A` itself need not be `Clone` for the handle --
// we only clone the `Sender` and `EventReader`, both of which are always
// `Clone` regardless of `A`.
impl<A: Aggregate> Clone for AggregateHandle<A> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            reader: self.reader.clone(),
        }
    }
}

impl<A: Aggregate> AggregateHandle<A> {
    /// Send a command to the aggregate and wait for the result.
    ///
    /// Returns the domain events produced by the command on success.
    ///
    /// # Arguments
    ///
    /// * `cmd` - The domain command to execute against the aggregate.
    /// * `ctx` - Cross-cutting metadata (actor identity, correlation ID, etc.).
    ///
    /// # Returns
    ///
    /// The domain events produced by the command on success.
    ///
    /// # Errors
    ///
    /// * [`ExecuteError::Domain`] -- the aggregate rejected the command.
    /// * [`ExecuteError::Io`] -- a disk I/O error occurred.
    /// * [`ExecuteError::ActorGone`] -- the actor thread has exited.
    pub async fn execute(
        &self,
        cmd: A::Command,
        ctx: CommandContext,
    ) -> Result<Vec<A::DomainEvent>, ExecuteError<A::Error>> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ActorMessage::Execute {
                cmd,
                ctx,
                reply: tx,
            })
            .await
            .map_err(|_| ExecuteError::ActorGone)?;
        rx.await.map_err(|_| ExecuteError::ActorGone)?
    }

    /// Read the current aggregate state.
    ///
    /// Refreshes the view from disk before returning.
    ///
    /// # Returns
    ///
    /// A clone of the current aggregate state.
    ///
    /// # Errors
    ///
    /// * [`StateError::Io`] -- a disk I/O error occurred.
    /// * [`StateError::ActorGone`] -- the actor thread has exited.
    pub async fn state(&self) -> Result<A, StateError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ActorMessage::GetState { reply: tx })
            .await
            .map_err(|_| StateError::ActorGone)?;
        rx.await.map_err(|_| StateError::ActorGone)?
    }

    /// Returns a reference to the [`EventReader`] for this aggregate's stream.
    pub fn reader(&self) -> &EventReader {
        &self.reader
    }
}

/// Spawn a new aggregate actor for the stream at `stream_dir`.
///
/// Opens the [`EventWriter`], creates an [`EventReader`] and aggregate
/// state [`View`], then starts the actor loop on a dedicated blocking
/// thread.
///
/// # Arguments
///
/// * `stream_dir` - Path to the directory containing the event log
///   (e.g. `<base>/streams/<type>/<id>`).
///
/// # Returns
///
/// An [`AggregateHandle`] for sending commands and reading state.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the event log cannot be opened.
pub fn spawn_actor<A: Aggregate>(stream_dir: &Path) -> io::Result<AggregateHandle<A>> {
    let writer = EventWriter::open(stream_dir)?;
    let reader = writer.reader();
    let views_dir = stream_dir.join("views");
    let view = View::<A>::new("state", reducer::<A>(), &views_dir);
    let (tx, rx) = mpsc::channel::<ActorMessage<A>>(32);
    let handle_reader = reader.clone();

    std::thread::spawn(move || {
        run_actor::<A>(writer, view, reader, rx);
    });

    Ok(AggregateHandle {
        sender: tx,
        reader: handle_reader,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::aggregate::test_fixtures::{Counter, CounterCommand, CounterError, CounterEvent};
    use crate::error::ExecuteError;

    #[tokio::test]
    async fn execute_increment_three_times() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");

        let ctx = CommandContext::default();
        for _ in 0..3 {
            handle
                .execute(CounterCommand::Increment, ctx.clone())
                .await
                .expect("execute should succeed");
        }

        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 3);
    }

    #[tokio::test]
    async fn execute_decrement_at_zero_returns_domain_error() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");

        let result = handle
            .execute(CounterCommand::Decrement, CommandContext::default())
            .await;

        assert!(
            matches!(result, Err(ExecuteError::Domain(CounterError::AlreadyZero))),
            "expected Domain(AlreadyZero), got: {result:?}"
        );
    }

    #[tokio::test]
    async fn state_persists_across_respawn() {
        let tmp = TempDir::new().expect("failed to create temp dir");

        // First actor: increment twice, then drop the handle.
        {
            let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");
            let ctx = CommandContext::default();
            handle
                .execute(CounterCommand::Increment, ctx.clone())
                .await
                .expect("first increment should succeed");
            handle
                .execute(CounterCommand::Increment, ctx)
                .await
                .expect("second increment should succeed");
        }
        // Handle dropped -- channel closes, actor exits.

        // Brief sleep to let the actor thread finish and release the
        // flock before we open a new writer on the same directory.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Second actor on the same directory should recover the state.
        let handle = spawn_actor::<Counter>(tmp.path()).expect("respawn should succeed");
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 2);
    }

    #[tokio::test]
    async fn sequential_commands_correct() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");
        let ctx = CommandContext::default();

        handle
            .execute(CounterCommand::Increment, ctx.clone())
            .await
            .expect("increment should succeed");
        handle
            .execute(CounterCommand::Add(10), ctx.clone())
            .await
            .expect("add should succeed");
        handle
            .execute(CounterCommand::Decrement, ctx)
            .await
            .expect("decrement should succeed");

        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 10);
    }

    #[tokio::test]
    async fn execute_returns_produced_events() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");

        let events = handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("execute should succeed");

        assert_eq!(events, vec![CounterEvent::Incremented]);
    }
}
