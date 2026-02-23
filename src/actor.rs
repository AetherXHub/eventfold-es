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
use std::time::Duration;

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

/// Configuration for the actor loop.
///
/// Internal to the crate -- callers configure idle timeout through
/// [`AggregateStoreBuilder::idle_timeout`](crate::AggregateStoreBuilder::idle_timeout).
pub(crate) struct ActorConfig {
    /// How long the actor waits for a message before shutting down.
    /// An effectively infinite value means the actor never idles out.
    pub idle_timeout: Duration,
}

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

    /// Inject a pre-validated `eventfold::Event` directly into the stream.
    ///
    /// The actor appends the event via its owned `EventWriter` and sends
    /// the I/O result back on `reply`. This keeps the actor as the sole
    /// writer for any stream that has a live actor.
    Inject {
        /// The pre-validated event to append as-is.
        event: eventfold::Event,
        /// Channel to send back the I/O result.
        reply: oneshot::Sender<io::Result<()>>,
    },

    /// Gracefully shut down the actor loop.
    #[allow(dead_code)] // Constructed only in tests.
    Shutdown,
}

/// Runs the aggregate actor loop on a blocking thread.
///
/// Owns the `EventWriter` and aggregate state `View`. Receives messages
/// from `AggregateHandle` via the mpsc channel and processes them sequentially.
/// The loop exits when the channel closes (all senders dropped), a
/// `Shutdown` message is received, or the idle timeout elapses. On exit the
/// `EventWriter` is dropped, releasing the flock.
///
/// # Arguments
///
/// * `writer` - Exclusive writer for the aggregate's event stream.
/// * `view` - Derived view that holds the current aggregate state.
/// * `reader` - Reader used to refresh the view before each operation.
/// * `rx` - Receiving end of the mpsc channel carrying `ActorMessage`s.
/// * `config` - Actor configuration (idle timeout).
pub(crate) fn run_actor<A: Aggregate>(
    mut writer: EventWriter,
    mut view: View<A>,
    reader: EventReader,
    mut rx: mpsc::Receiver<ActorMessage<A>>,
    config: ActorConfig,
) {
    // Build a lightweight current-thread runtime with time enabled.
    // The actor needs `tokio::time::timeout` to implement idle eviction,
    // but the parent runtime may be current-thread (common in tests),
    // which doesn't drive timers from non-runtime threads. A dedicated
    // minimal runtime avoids that constraint and keeps the actor
    // self-contained.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("failed to create actor timeout runtime");

    loop {
        // Create the timeout future INSIDE `block_on` so that the `Sleep`
        // timer registers with the local runtime's time driver.
        let idle_timeout = config.idle_timeout;
        let msg = rt.block_on(async { tokio::time::timeout(idle_timeout, rx.recv()).await });

        match msg {
            // Received a message before the timeout elapsed.
            Ok(Some(msg)) => match msg {
                ActorMessage::Execute { cmd, ctx, reply } => {
                    let _span = tracing::info_span!("execute", aggregate_type = A::AGGREGATE_TYPE,)
                        .entered();
                    let result = execute_command::<A>(&mut writer, &mut view, &reader, cmd, &ctx);
                    // If the receiver was dropped, the caller no longer cares
                    // about the result. Silently discard it.
                    let _ = reply.send(result);
                }

                ActorMessage::GetState { reply } => {
                    let result = get_state::<A>(&mut view, &reader);
                    let _ = reply.send(result);
                }

                ActorMessage::Inject { event, reply } => {
                    let result = writer.append(&event).map(|_| ());
                    let _ = reply.send(result);
                }

                ActorMessage::Shutdown => break,
            },
            // Channel closed: all senders dropped.
            Ok(None) => break,
            // Idle timeout elapsed with no messages.
            Err(_elapsed) => {
                tracing::info!(
                    aggregate_type = A::AGGREGATE_TYPE,
                    "actor idle, shutting down"
                );
                break;
            }
        }
    }
    // Loop exited: either `Shutdown` received, channel closed, or idle timeout.
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

    tracing::info!(count = domain_events.len(), "events appended");

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

    /// Inject a pre-validated event by sending it to the actor for append.
    ///
    /// The actor owns the exclusive `EventWriter`, so this method routes
    /// the event through the actor's channel to avoid lock contention.
    /// The actor appends the event and sends the I/O result back.
    ///
    /// # Arguments
    ///
    /// * `event` - A pre-validated `eventfold::Event` to append as-is.
    ///
    /// # Errors
    ///
    /// * [`io::Error`] with [`io::ErrorKind::BrokenPipe`] if the actor
    ///   has exited and the channel is closed.
    /// * [`io::Error`] if the underlying `EventWriter::append` fails.
    pub(crate) async fn inject_via_actor(&self, event: eventfold::Event) -> io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ActorMessage::Inject { event, reply: tx })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "actor gone"))?;
        rx.await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "actor gone"))?
    }

    /// Returns a reference to the [`EventReader`] for this aggregate's stream.
    pub fn reader(&self) -> &EventReader {
        &self.reader
    }

    /// Check whether the actor backing this handle is still running.
    ///
    /// Returns `false` if the actor thread has exited (e.g. due to idle
    /// timeout or shutdown). The store uses this to evict stale handles
    /// from its cache and re-spawn the actor on the next `get` call.
    pub fn is_alive(&self) -> bool {
        !self.sender.is_closed()
    }
}

/// Spawn a new aggregate actor with explicit configuration.
///
/// This is the internal entry point used by [`AggregateStore`](crate::AggregateStore)
/// to pass an idle timeout to the actor loop.
///
/// # Arguments
///
/// * `stream_dir` - Path to the directory containing the event log.
/// * `config` - Actor configuration (idle timeout).
///
/// # Returns
///
/// An [`AggregateHandle`] for sending commands and reading state.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the event log cannot be opened.
pub(crate) fn spawn_actor_with_config<A: Aggregate>(
    stream_dir: &Path,
    config: ActorConfig,
) -> io::Result<AggregateHandle<A>> {
    let writer = EventWriter::open(stream_dir)?;
    let reader = writer.reader();
    let views_dir = stream_dir.join("views");
    let view = View::<A>::new("state", reducer::<A>(), &views_dir);
    let (tx, rx) = mpsc::channel::<ActorMessage<A>>(32);
    let handle_reader = reader.clone();

    std::thread::spawn(move || {
        run_actor::<A>(writer, view, reader, rx, config);
    });

    Ok(AggregateHandle {
        sender: tx,
        reader: handle_reader,
    })
}

/// Spawn a new aggregate actor for the stream at `stream_dir`.
///
/// Opens the [`EventWriter`], creates an [`EventReader`] and aggregate
/// state [`View`], then starts the actor loop on a dedicated blocking
/// thread.
///
/// The actor created by this function uses an effectively infinite idle
/// timeout. For configurable timeouts, use
/// [`AggregateStoreBuilder::idle_timeout`](crate::AggregateStoreBuilder::idle_timeout).
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
    // Use an effectively infinite timeout so the actor never idles out.
    // `u64::MAX / 2` avoids overflow when tokio adds the timeout duration
    // to the current `Instant`.
    let config = ActorConfig {
        idle_timeout: Duration::from_secs(u64::MAX / 2),
    };
    spawn_actor_with_config(stream_dir, config)
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

    #[tokio::test]
    async fn idle_timeout_shuts_down_actor() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let config = ActorConfig {
            idle_timeout: Duration::from_millis(200),
        };
        let handle =
            spawn_actor_with_config::<Counter>(tmp.path(), config).expect("spawn should succeed");

        // Execute a command before the timeout.
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("first execute should succeed");

        // Wait for idle timeout to elapse.
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Actor should have shut down.
        assert!(
            !handle.is_alive(),
            "actor should be dead after idle timeout"
        );

        // Re-spawn on the same directory recovers state from disk.
        let config2 = ActorConfig {
            idle_timeout: Duration::from_secs(u64::MAX / 2),
        };
        let handle2 = spawn_actor_with_config::<Counter>(tmp.path(), config2)
            .expect("respawn should succeed");
        let state = handle2.state().await.expect("state should succeed");
        assert_eq!(state.value, 1, "state should reflect the first command");
    }

    #[tokio::test]
    async fn inject_via_actor_appends_and_updates_state() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let handle = spawn_actor::<Counter>(tmp.path()).expect("spawn_actor should succeed");

        // Build a raw eventfold::Event matching the Counter's "Incremented" variant.
        let event = eventfold::Event::new("Incremented", serde_json::Value::Null);

        // Inject the event directly via the actor.
        handle
            .inject_via_actor(event)
            .await
            .expect("inject_via_actor should succeed");

        // The actor's view should pick up the injected event on next refresh.
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(
            state.value, 1,
            "injected Incremented event should bump counter to 1"
        );
    }

    #[tokio::test]
    async fn rapid_commands_prevent_idle_eviction() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let config = ActorConfig {
            idle_timeout: Duration::from_millis(300),
        };
        let handle =
            spawn_actor_with_config::<Counter>(tmp.path(), config).expect("spawn should succeed");

        let ctx = CommandContext::default();
        // Send commands at 100ms intervals, each resetting the idle timer.
        for _ in 0..5 {
            handle
                .execute(CounterCommand::Increment, ctx.clone())
                .await
                .expect("execute should succeed");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        assert!(
            handle.is_alive(),
            "actor should still be alive during activity"
        );
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 5);
    }
}
