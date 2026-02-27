//! Cross-aggregate workflow coordination (process managers).
//!
//! A process manager reacts to events from the global event log and
//! produces [`CommandEnvelope`]s that are dispatched to (potentially
//! different) aggregates. They are structurally similar to projections
//! -- they use a global cursor for catch-up -- but produce side effects
//! (commands) rather than read models.
//!
//! Unlike projections, `catch_up` does **not** save the checkpoint.
//! The caller dispatches the returned envelopes first, then calls
//! [`ProcessManagerRunner::save`] to persist the checkpoint. This
//! ensures crash safety: a crash mid-dispatch causes re-processing
//! on restart.

use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

use crate::command::CommandEnvelope;
use crate::event::{StoredEvent, decode_stored_event};
use crate::proto::{self, subscribe_response};

/// A cross-aggregate workflow coordinator that reacts to events by
/// producing commands.
///
/// Process managers consume events from all aggregate streams via the
/// gRPC `SubscribeAll` endpoint and emit [`CommandEnvelope`]s for
/// dispatch. Subscription filtering is done in the [`react`](ProcessManager::react)
/// body by inspecting `event.aggregate_type` or `event.event_type`.
///
/// # Contract
///
/// - [`react`](ProcessManager::react) must be deterministic: given the
///   same sequence of events, it must produce the same command envelopes.
/// - Unknown event types or aggregate types should be silently ignored
///   for forward compatibility.
/// - State is checkpointed after all envelopes from a catch-up pass
///   have been dispatched (or dead-lettered), ensuring crash safety
///   via re-processing.
pub trait ProcessManager: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Human-readable name, used as a directory name for checkpoints.
    const NAME: &'static str;

    /// React to a single event from the global log.
    ///
    /// Returns zero or more [`CommandEnvelope`]s to dispatch. The event
    /// carries `aggregate_type`, `instance_id`, `event_type`, and all
    /// other fields pre-extracted from the gRPC `RecordedEvent`.
    /// Implementors filter on whichever fields they need in the body.
    ///
    /// # Arguments
    ///
    /// * `event` - A reference to the decoded [`StoredEvent`].
    ///
    /// # Returns
    ///
    /// A `Vec` of command envelopes to dispatch. Empty if this event
    /// is irrelevant to the process manager.
    fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope>;
}

/// Persisted state of a process manager including the global cursor position.
///
/// Serialized to JSON as `{ "state": <PM>, "last_global_position": <N> }`.
/// The `last_global_position` field is a resume token: the next global
/// position to read from. A value of `0` means "start from the beginning."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProcessManagerCheckpoint<PM> {
    /// The process manager's current state.
    pub state: PM,
    /// Resume token: the next global position to read from.
    ///
    /// After processing an event at global position N, this is set to N + 1.
    /// A value of 0 means no events have been processed yet.
    pub last_global_position: u64,
}

impl<PM: Default> Default for ProcessManagerCheckpoint<PM> {
    fn default() -> Self {
        Self {
            state: PM::default(),
            last_global_position: 0,
        }
    }
}

// --- Checkpoint persistence helpers ---

/// Save a process manager checkpoint atomically.
///
/// Writes to a temporary file then renames to `checkpoint.json` in `dir`.
/// Creates `dir` if it does not exist.
///
/// # Arguments
///
/// * `dir` - Directory to store the checkpoint file in.
/// * `checkpoint` - The checkpoint to persist.
///
/// # Errors
///
/// Returns `io::Error` if directory creation, file writing, or renaming fails.
pub(crate) fn save_pm_checkpoint<PM: ProcessManager>(
    dir: &Path,
    checkpoint: &ProcessManagerCheckpoint<PM>,
) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("checkpoint.json");
    let tmp_path = dir.join("checkpoint.json.tmp");
    let json = serde_json::to_string_pretty(checkpoint).map_err(io::Error::other)?;
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Load a process manager checkpoint from disk.
///
/// Returns `Ok(None)` if the file does not exist or is corrupt.
/// A corrupt checkpoint is not a hard error -- the process manager will rebuild.
///
/// # Arguments
///
/// * `dir` - Directory containing the `checkpoint.json` file.
///
/// # Errors
///
/// Returns `io::Error` for I/O failures other than file-not-found.
pub(crate) fn load_pm_checkpoint<PM: ProcessManager>(
    dir: &Path,
) -> io::Result<Option<ProcessManagerCheckpoint<PM>>> {
    let path = dir.join("checkpoint.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(checkpoint) => Ok(Some(checkpoint)),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "corrupt process manager checkpoint, will rebuild"
                );
                Ok(None)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// --- ProcessManagerRunner ---

/// Drives a process manager's catch-up loop, reading events from the
/// global log and collecting command envelopes for dispatch.
///
/// Manages the lifecycle of a single [`ProcessManager`]: loading its
/// persisted checkpoint, catching up on new events via `SubscribeAll`,
/// and allowing the caller to save the checkpoint after dispatch.
///
/// Unlike [`ProjectionRunner`](crate::projection::ProjectionRunner),
/// `catch_up` does **not** persist the checkpoint. Instead it returns
/// the collected envelopes, and the caller must invoke
/// [`save`](ProcessManagerRunner::save) after all envelopes have been
/// dispatched (or dead-lettered). This ensures crash safety: a crash
/// mid-dispatch causes re-processing on restart.
pub(crate) struct ProcessManagerRunner<PM: ProcessManager> {
    /// The process manager's persisted state and global cursor position.
    checkpoint: ProcessManagerCheckpoint<PM>,
    /// gRPC client for subscribing to the global log.
    client: crate::client::EsClient,
    /// Directory where this process manager's checkpoint file is stored.
    checkpoint_dir: PathBuf,
}

impl<PM: ProcessManager> ProcessManagerRunner<PM> {
    /// Create a new runner, loading an existing checkpoint from disk if available.
    ///
    /// If no checkpoint file exists, the runner starts with a default
    /// (empty) process manager state at global position 0.
    ///
    /// # Arguments
    ///
    /// * `client` - The gRPC client for subscribing to the global event log.
    /// * `checkpoint_dir` - Directory where this process manager's checkpoint
    ///   file is stored.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if reading an existing checkpoint file fails
    /// (other than file-not-found).
    pub(crate) fn new(
        client: crate::client::EsClient,
        checkpoint_dir: PathBuf,
    ) -> io::Result<Self> {
        let checkpoint = load_pm_checkpoint::<PM>(&checkpoint_dir)?.unwrap_or_default();
        Ok(Self {
            checkpoint,
            client,
            checkpoint_dir,
        })
    }

    /// Returns the current process manager state.
    #[allow(dead_code)] // Public API for callers inspecting runner state.
    pub(crate) fn state(&self) -> &PM {
        &self.checkpoint.state
    }

    /// Returns the current global cursor position (resume token).
    #[allow(dead_code)] // Public API for callers inspecting runner state.
    pub(crate) fn position(&self) -> u64 {
        self.checkpoint.last_global_position
    }

    /// Catch up on new events from the global log.
    ///
    /// Subscribes to the global event log starting at the current cursor
    /// position, reads until a `CaughtUp` message, decodes and reacts to
    /// each event, and collects the resulting command envelopes.
    ///
    /// Does **not** save the checkpoint. The caller must invoke
    /// [`save`](ProcessManagerRunner::save) after dispatching all
    /// returned envelopes.
    ///
    /// Events with missing or unparseable metadata (non-eventfold-es events)
    /// are silently skipped.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the gRPC subscription fails or the stream
    /// yields an error.
    pub(crate) async fn catch_up(&mut self) -> io::Result<Vec<CommandEnvelope>> {
        let stream = self
            .client
            .subscribe_all_from(self.checkpoint.last_global_position)
            .await
            .map_err(|e| io::Error::other(format!("subscribe_all_from failed: {e}")))?;

        Self::process_stream(&mut self.checkpoint, stream).await
    }

    /// Process a stream of `SubscribeResponse` messages, updating the checkpoint.
    ///
    /// Reads from the stream until a `CaughtUp` message is received. For each
    /// `RecordedEvent`, attempts to decode it via [`decode_stored_event`]. Events
    /// that cannot be decoded (missing metadata, invalid UUIDs, etc.) are silently
    /// skipped. Successfully decoded events are passed to
    /// [`ProcessManager::react`] and the resulting envelopes are collected.
    ///
    /// This function is factored out of [`catch_up`] so that tests can provide
    /// a mock stream without needing a live gRPC server.
    async fn process_stream(
        checkpoint: &mut ProcessManagerCheckpoint<PM>,
        mut stream: impl tokio_stream::Stream<Item = Result<proto::SubscribeResponse, tonic::Status>>
        + Unpin,
    ) -> io::Result<Vec<CommandEnvelope>> {
        tracing::debug!(pm_name = PM::NAME, "starting process manager catch-up");

        let mut envelopes = Vec::new();

        while let Some(result) = stream.next().await {
            let response =
                result.map_err(|e| io::Error::other(format!("subscribe stream error: {e}")))?;

            match response.content {
                Some(subscribe_response::Content::Event(recorded)) => {
                    // Record the global position for cursor advancement regardless
                    // of whether decoding succeeds, so we skip past non-ES events.
                    let next_position = recorded.global_position + 1;

                    if let Some(stored) = decode_stored_event(&recorded) {
                        let produced = checkpoint.state.react(&stored);
                        envelopes.extend(produced);
                        tracing::debug!(
                            global_position = stored.global_position,
                            event_type = %stored.event_type,
                            envelopes_produced = envelopes.len(),
                            "event reacted"
                        );
                    }

                    checkpoint.last_global_position = next_position;
                }
                Some(subscribe_response::Content::CaughtUp(_)) => {
                    tracing::debug!("caught up");
                    break;
                }
                None => {
                    // Empty response content; skip.
                }
            }
        }

        Ok(envelopes)
    }

    /// Persist the current checkpoint to disk.
    ///
    /// Should be called after all envelopes from a catch-up pass have been
    /// dispatched (or dead-lettered).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if writing the checkpoint fails.
    pub(crate) fn save(&self) -> io::Result<()> {
        save_pm_checkpoint::<PM>(&self.checkpoint_dir, &self.checkpoint)
    }

    /// Returns the process manager name.
    #[allow(dead_code)] // Used by ProcessManagerCatchUp trait impl.
    pub(crate) fn name(&self) -> &str {
        PM::NAME
    }

    /// Returns the path to this process manager's dead-letter log.
    pub(crate) fn dead_letter_path(&self) -> PathBuf {
        self.checkpoint_dir.join("dead_letters.jsonl")
    }
}

// --- Type-erased trait for store integration ---

/// Trait object interface for process manager runners.
///
/// Allows `run_process_managers` to iterate over heterogeneous process
/// managers without knowing each concrete `PM` type. All methods are
/// async-compatible via boxed futures.
pub(crate) trait ProcessManagerCatchUp: Send + Sync {
    /// Catch up on the global event log and return command envelopes.
    ///
    /// Does **not** persist the checkpoint.
    fn catch_up(
        &mut self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = io::Result<Vec<CommandEnvelope>>> + Send + '_>,
    >;

    /// Persist the checkpoint to disk.
    fn save(&self) -> io::Result<()>;

    /// Returns the process manager name.
    #[allow(dead_code)] // API surface for logging/diagnostics during dispatch.
    fn name(&self) -> &str;

    /// Returns the path to the dead-letter log file.
    fn dead_letter_path(&self) -> PathBuf;
}

impl<PM: ProcessManager> ProcessManagerCatchUp for ProcessManagerRunner<PM> {
    fn catch_up(
        &mut self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = io::Result<Vec<CommandEnvelope>>> + Send + '_>,
    > {
        Box::pin(self.catch_up())
    }

    fn save(&self) -> io::Result<()> {
        self.save()
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn dead_letter_path(&self) -> PathBuf {
        self.dead_letter_path()
    }
}

// --- Dead-letter log ---

/// An entry in the dead-letter log, recording a failed dispatch attempt.
#[derive(Debug, Serialize, Deserialize)]
struct DeadLetterEntry {
    /// The command envelope that failed to dispatch.
    envelope: CommandEnvelope,
    /// Human-readable error message.
    error: String,
    /// Unix timestamp (seconds since epoch) of the failure.
    ts: u64,
}

/// Append a single dead-letter entry to the JSONL log at `path`.
///
/// Creates the file if it does not exist. Each entry is a single JSON line.
///
/// # Errors
///
/// Returns `io::Error` if file I/O fails.
pub(crate) fn append_dead_letter(
    path: &Path,
    envelope: CommandEnvelope,
    error: &str,
) -> io::Result<()> {
    use std::io::Write;
    let ts = std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .expect("system clock is before Unix epoch")
        .as_secs();
    let entry = DeadLetterEntry {
        envelope,
        error: error.to_string(),
        ts,
    };
    let json = serde_json::to_string(&entry).map_err(io::Error::other)?;
    // Ensure the parent directory exists (the PM checkpoint dir may not
    // have been created yet if no catch_up save has occurred).
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{json}")?;
    Ok(())
}

/// Summary of a `run_process_managers` pass.
#[derive(Debug, Clone, Default)]
pub struct ProcessManagerReport {
    /// Number of command envelopes successfully dispatched.
    pub dispatched: usize,
    /// Number of command envelopes written to dead-letter logs.
    pub dead_lettered: usize,
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;
    use crate::command::CommandContext;

    /// A test process manager that reacts to counter events by emitting
    /// a command envelope targeting a "target" aggregate.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub(crate) struct EchoSaga {
        /// Number of events processed (for testing state persistence).
        pub events_seen: u64,
    }

    impl ProcessManager for EchoSaga {
        const NAME: &'static str = "echo-saga";

        fn react(&mut self, event: &StoredEvent) -> Vec<CommandEnvelope> {
            // Only react to "counter" aggregate events.
            if event.aggregate_type != "counter" {
                return Vec::new();
            }
            self.events_seen += 1;
            vec![CommandEnvelope {
                aggregate_type: "target".to_string(),
                instance_id: event.instance_id.clone(),
                command: serde_json::json!({
                    "source_event_type": event.event_type,
                }),
                context: CommandContext::default()
                    .with_correlation_id(format!("echo-{}", self.events_seen)),
            }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::EchoSaga;

    use crate::command::CommandContext;
    use crate::proto::{Empty, RecordedEvent, SubscribeResponse, subscribe_response::Content};

    // --- Helper functions for building mock stream responses ---

    /// Build a `RecordedEvent` with valid eventfold-es metadata.
    fn make_recorded_event(global_position: u64, stream_version: u64) -> RecordedEvent {
        let event_id = uuid::Uuid::new_v4().to_string();
        let stream_id = crate::event::stream_uuid("counter", "c-1").to_string();
        let metadata = serde_json::json!({
            "aggregate_type": "counter",
            "instance_id": "c-1"
        });
        RecordedEvent {
            event_id,
            stream_id,
            stream_version,
            global_position,
            event_type: "Incremented".to_string(),
            metadata: serde_json::to_vec(&metadata).expect("serialize metadata"),
            payload: b"{}".to_vec(),
            recorded_at: 1_700_000_000_000,
        }
    }

    /// Build a `SubscribeResponse` wrapping a `RecordedEvent`.
    fn event_response(
        global_position: u64,
        stream_version: u64,
    ) -> Result<SubscribeResponse, tonic::Status> {
        Ok(SubscribeResponse {
            content: Some(Content::Event(make_recorded_event(
                global_position,
                stream_version,
            ))),
        })
    }

    /// Build a `SubscribeResponse` with the `CaughtUp` sentinel.
    fn caught_up_response() -> Result<SubscribeResponse, tonic::Status> {
        Ok(SubscribeResponse {
            content: Some(Content::CaughtUp(Empty {})),
        })
    }

    // --- Trait shape tests ---

    #[test]
    fn process_manager_trait_has_no_subscriptions_method() {
        // Verify the new trait shape: NAME + react(&mut self, &StoredEvent).
        let mut saga = EchoSaga::default();
        let stored = StoredEvent {
            event_id: uuid::Uuid::new_v4(),
            stream_id: uuid::Uuid::new_v4(),
            aggregate_type: "counter".to_string(),
            instance_id: "c-1".to_string(),
            stream_version: 0,
            global_position: 0,
            event_type: "Incremented".to_string(),
            payload: serde_json::json!({}),
            metadata: serde_json::json!({}),
            recorded_at: 1_700_000_000_000,
        };
        let envelopes = saga.react(&stored);
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].aggregate_type, "target");
        assert_eq!(envelopes[0].instance_id, "c-1");
        assert_eq!(envelopes[0].command["source_event_type"], "Incremented");
        assert_eq!(saga.events_seen, 1);
    }

    // --- Checkpoint tests ---

    #[test]
    fn checkpoint_serialization_roundtrip() {
        let checkpoint = ProcessManagerCheckpoint {
            state: EchoSaga { events_seen: 5 },
            last_global_position: 42,
        };
        let json = serde_json::to_string(&checkpoint).expect("serialization should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(parsed["state"]["events_seen"], 5);
        assert_eq!(parsed["last_global_position"], 42);

        // Full roundtrip.
        let loaded: ProcessManagerCheckpoint<EchoSaga> =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.last_global_position, checkpoint.last_global_position);
    }

    #[test]
    fn checkpoint_default_has_position_zero() {
        let checkpoint = ProcessManagerCheckpoint::<EchoSaga>::default();
        assert_eq!(checkpoint.state.events_seen, 0);
        assert_eq!(checkpoint.last_global_position, 0);
    }

    // --- Checkpoint persistence tests ---

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint = ProcessManagerCheckpoint {
            state: EchoSaga { events_seen: 7 },
            last_global_position: 99,
        };

        save_pm_checkpoint(dir.path(), &checkpoint).expect("save should succeed");
        let loaded: ProcessManagerCheckpoint<EchoSaga> = load_pm_checkpoint(dir.path())
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.last_global_position, checkpoint.last_global_position);
    }

    #[test]
    fn load_from_empty_dir_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let result = load_pm_checkpoint::<EchoSaga>(dir.path()).expect("load should not error");
        assert!(result.is_none());
    }

    #[test]
    fn corrupt_file_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        std::fs::write(dir.path().join("checkpoint.json"), "not valid json!!!")
            .expect("write should succeed");
        let loaded = load_pm_checkpoint::<EchoSaga>(dir.path()).expect("load should not error");
        assert!(loaded.is_none());
    }

    // --- process_stream / catch_up tests ---

    #[tokio::test]
    async fn catch_up_with_one_valid_event_returns_one_envelope() {
        let mut checkpoint = ProcessManagerCheckpoint::<EchoSaga>::default();
        let stream = tokio_stream::iter(vec![event_response(0, 0), caught_up_response()]);

        let envelopes = ProcessManagerRunner::<EchoSaga>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");

        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].aggregate_type, "target");
        assert_eq!(envelopes[0].instance_id, "c-1");
        assert_eq!(checkpoint.state.events_seen, 1);
        // Resume token should be global_position + 1 = 1.
        assert_eq!(checkpoint.last_global_position, 1);
    }

    #[tokio::test]
    async fn second_catch_up_with_saved_checkpoint_returns_empty() {
        // First pass: process one event.
        let mut checkpoint = ProcessManagerCheckpoint::<EchoSaga>::default();
        let stream = tokio_stream::iter(vec![event_response(0, 0), caught_up_response()]);

        let envelopes = ProcessManagerRunner::<EchoSaga>::process_stream(&mut checkpoint, stream)
            .await
            .expect("first process_stream should succeed");
        assert_eq!(envelopes.len(), 1);

        // Simulate: caller saves checkpoint, then no new events on second pass.
        let stream = tokio_stream::iter(vec![caught_up_response()]);

        let envelopes = ProcessManagerRunner::<EchoSaga>::process_stream(&mut checkpoint, stream)
            .await
            .expect("second process_stream should succeed");
        assert!(envelopes.is_empty());
        assert_eq!(checkpoint.state.events_seen, 1);
        assert_eq!(checkpoint.last_global_position, 1);
    }

    #[tokio::test]
    async fn non_es_events_skipped_returns_empty() {
        // RecordedEvent with metadata = b"{}" has no aggregate_type or instance_id,
        // so decode_stored_event returns None and the event is skipped.
        let recorded = RecordedEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            stream_id: uuid::Uuid::new_v4().to_string(),
            stream_version: 0,
            global_position: 5,
            event_type: "SomeEvent".to_string(),
            metadata: b"{}".to_vec(),
            payload: b"{}".to_vec(),
            recorded_at: 1_700_000_000_000,
        };
        let stream = tokio_stream::iter(vec![
            Ok(SubscribeResponse {
                content: Some(Content::Event(recorded)),
            }),
            caught_up_response(),
        ]);
        let mut checkpoint = ProcessManagerCheckpoint::<EchoSaga>::default();

        let envelopes = ProcessManagerRunner::<EchoSaga>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");

        // Event was skipped, no envelopes produced.
        assert!(envelopes.is_empty());
        assert_eq!(checkpoint.state.events_seen, 0);
        // But position should still advance past the skipped event.
        assert_eq!(checkpoint.last_global_position, 6);
    }

    // --- Dead-letter tests ---

    #[test]
    fn dead_letter_append_creates_readable_jsonl() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let path = tmp.path().join("dead_letters.jsonl");

        let envelope = CommandEnvelope {
            aggregate_type: "target".to_string(),
            instance_id: "t-1".to_string(),
            command: serde_json::json!({"action": "test"}),
            context: CommandContext::default(),
        };

        append_dead_letter(&path, envelope, "test error").expect("append should succeed");

        let contents = std::fs::read_to_string(&path).expect("read should succeed");
        let entry: DeadLetterEntry =
            serde_json::from_str(contents.trim()).expect("should be valid JSON");
        assert_eq!(entry.error, "test error");
        assert_eq!(entry.envelope.aggregate_type, "target");
    }
}
