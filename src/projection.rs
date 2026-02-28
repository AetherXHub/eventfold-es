//! Cross-stream projections (read models) backed by the global event log.
//!
//! Projections consume events from all aggregate streams via the gRPC
//! `SubscribeAll` endpoint. Each projection maintains a single global
//! cursor position instead of per-stream byte offsets, simplifying
//! catch-up and checkpointing.

use std::any::Any;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

use crate::event::{StoredEvent, decode_stored_event};
use crate::proto::{self, subscribe_response};

/// A cross-stream read model that consumes events from the global log.
///
/// Projections are eventually consistent: they catch up by reading new
/// events from the global log via `SubscribeAll` and are rebuilt from
/// scratch if their checkpoint is invalid.
///
/// # Contract
///
/// - [`apply`](Projection::apply) must be deterministic: given the same
///   sequence of events, it must produce the same state.
/// - Unknown event types or aggregate types should be silently ignored
///   for forward compatibility. Filtering by `event.aggregate_type` or
///   `event.event_type` is done in the method body.
pub trait Projection:
    Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static
{
    /// Human-readable name, used as a directory name for checkpoints.
    const NAME: &'static str;

    /// Apply a single event from the global log.
    ///
    /// The event carries `aggregate_type`, `instance_id`, `event_type`,
    /// and all other fields pre-extracted from the gRPC `RecordedEvent`.
    /// Implementors filter on whichever fields they need in the body.
    fn apply(&mut self, event: &StoredEvent);
}

/// Persisted state of a projection including the global cursor position.
///
/// Serialized to JSON as `{ "state": <P>, "last_global_position": <N> }`.
/// The `last_global_position` field is a resume token: the next global
/// position to read from. A value of `0` means "start from the beginning."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectionCheckpoint<P> {
    /// The projection's current state.
    pub state: P,
    /// Resume token: the next global position to read from.
    ///
    /// After processing an event at global position N, this is set to N + 1.
    /// A value of 0 means no events have been processed yet.
    pub last_global_position: u64,
}

impl<P: Default> Default for ProjectionCheckpoint<P> {
    fn default() -> Self {
        Self {
            state: P::default(),
            last_global_position: 0,
        }
    }
}

/// Save a projection checkpoint atomically.
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
pub(crate) fn save_checkpoint<P: Projection>(
    dir: &Path,
    checkpoint: &ProjectionCheckpoint<P>,
) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("checkpoint.json");
    let tmp_path = dir.join("checkpoint.json.tmp");
    let json = serde_json::to_string_pretty(checkpoint).map_err(io::Error::other)?;
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

/// Load a projection checkpoint from disk.
///
/// Returns `Ok(None)` if the file does not exist or is corrupt.
/// A corrupt checkpoint is not a hard error -- the projection will rebuild.
///
/// # Arguments
///
/// * `dir` - Directory containing the `checkpoint.json` file.
///
/// # Errors
///
/// Returns `io::Error` for I/O failures other than file-not-found.
pub(crate) fn load_checkpoint<P: Projection>(
    dir: &Path,
) -> io::Result<Option<ProjectionCheckpoint<P>>> {
    let path = dir.join("checkpoint.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(checkpoint) => Ok(Some(checkpoint)),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "corrupt projection checkpoint, will rebuild"
                );
                Ok(None)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Drives a projection's catch-up loop, reading events from the global log.
///
/// Manages the lifecycle of a single [`Projection`]: loading its persisted
/// checkpoint, catching up on new events via `SubscribeAll`, and saving
/// the checkpoint back to disk.
pub(crate) struct ProjectionRunner<P: Projection> {
    /// The projection's persisted state and global cursor position.
    checkpoint: ProjectionCheckpoint<P>,
    /// gRPC client for subscribing to the global log.
    client: crate::client::EsClient,
    /// Directory where this projection's checkpoint file is stored.
    checkpoint_dir: PathBuf,
}

impl<P: Projection> ProjectionRunner<P> {
    /// Create a new runner, loading an existing checkpoint from disk if available.
    ///
    /// If no checkpoint file exists, the runner starts with a default
    /// (empty) projection state at global position 0.
    ///
    /// # Arguments
    ///
    /// * `client` - The gRPC client for subscribing to the global event log.
    /// * `checkpoint_dir` - Directory where this projection's checkpoint file
    ///   is stored.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if reading an existing checkpoint file fails
    /// (other than file-not-found).
    pub(crate) fn new(
        client: crate::client::EsClient,
        checkpoint_dir: PathBuf,
    ) -> io::Result<Self> {
        let checkpoint = load_checkpoint::<P>(&checkpoint_dir)?.unwrap_or_default();
        Ok(Self {
            checkpoint,
            client,
            checkpoint_dir,
        })
    }

    /// Returns the current projection state.
    #[allow(dead_code)] // Superseded by ProjectionCatchUp::state_any for store access.
    pub(crate) fn state(&self) -> &P {
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
    /// position, reads until a `CaughtUp` message, decodes and applies
    /// each event, and saves the updated checkpoint to disk.
    ///
    /// Events with missing or unparseable metadata (non-eventfold-es events)
    /// are silently skipped.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the gRPC subscription fails, the stream
    /// yields an error, or saving the checkpoint fails.
    pub(crate) async fn catch_up(&mut self) -> io::Result<()> {
        let stream = self
            .client
            .subscribe_all_from(self.checkpoint.last_global_position)
            .await
            .map_err(|e| io::Error::other(format!("subscribe_all_from failed: {e}")))?;

        Self::process_stream(&mut self.checkpoint, stream).await?;
        save_checkpoint::<P>(&self.checkpoint_dir, &self.checkpoint)?;
        Ok(())
    }

    /// Process a stream of `SubscribeResponse` messages, updating the checkpoint.
    ///
    /// Reads from the stream until a `CaughtUp` message is received. For each
    /// `RecordedEvent`, attempts to decode it via [`decode_stored_event`]. Events
    /// that cannot be decoded (missing metadata, invalid UUIDs, etc.) are silently
    /// skipped. Successfully decoded events are applied to the projection.
    ///
    /// This function is factored out of [`catch_up`] so that tests can provide
    /// a mock stream without needing a live gRPC server.
    async fn process_stream(
        checkpoint: &mut ProjectionCheckpoint<P>,
        mut stream: impl tokio_stream::Stream<Item = Result<proto::SubscribeResponse, tonic::Status>>
        + Unpin,
    ) -> io::Result<()> {
        tracing::debug!(projection_name = P::NAME, "starting projection catch-up");

        while let Some(result) = stream.next().await {
            let response =
                result.map_err(|e| io::Error::other(format!("subscribe stream error: {e}")))?;

            match response.content {
                Some(subscribe_response::Content::Event(recorded)) => {
                    apply_recorded_event(
                        &mut checkpoint.state,
                        &mut checkpoint.last_global_position,
                        &recorded,
                    );
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

        Ok(())
    }
}

/// Decode a single [`proto::RecordedEvent`] and apply it to a projection's state,
/// advancing the checkpoint position.
///
/// This helper is shared between [`ProjectionRunner::process_stream`] (batch
/// catch-up) and [`ProjectionCatchUp::apply_event`] (single-event live mode).
/// Events that cannot be decoded (missing metadata, invalid UUIDs, etc.) are
/// silently skipped, but the position always advances.
fn apply_recorded_event<P: Projection>(
    state: &mut P,
    last_global_position: &mut u64,
    recorded: &proto::RecordedEvent,
) {
    // Skip events already processed. This guards against replay when the live
    // loop subscribes from min_global_position across multiple projections --
    // projections ahead of the minimum would otherwise re-apply events.
    if recorded.global_position < *last_global_position {
        return;
    }

    let next_position = recorded.global_position + 1;

    if let Some(stored) = decode_stored_event(recorded) {
        state.apply(&stored);
        tracing::debug!(
            global_position = stored.global_position,
            event_type = %stored.event_type,
            "event applied"
        );
    }

    *last_global_position = next_position;
}

// --- Type-erased trait for store integration ---

/// Type-erased interface for projection runners.
///
/// Allows the live subscription loop and `AggregateStore` to interact with
/// heterogeneous projections without knowing each concrete `P` type. All
/// async methods use boxed futures for trait-object compatibility.
pub(crate) trait ProjectionCatchUp: Send + Sync {
    /// Catch up on the global event log by subscribing from the current
    /// checkpoint position.
    fn catch_up(&mut self) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>>;

    /// Decode and apply a single recorded event, advancing the checkpoint
    /// position.
    ///
    /// Used by the live subscription loop to process events one at a time
    /// rather than draining a full stream.
    fn apply_event(&mut self, recorded: &proto::RecordedEvent);

    /// Returns the current global cursor position (resume token).
    fn position(&self) -> u64;

    /// Persist the current checkpoint to disk.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if writing the checkpoint fails.
    fn save(&self) -> io::Result<()>;

    /// Clone the current projection state into a type-erased box.
    ///
    /// The caller can downcast the returned `Box<dyn Any>` to the concrete
    /// projection type `P` to read the state.
    fn state_any(&self) -> Box<dyn Any + Send>;
}

impl<P: Projection> ProjectionCatchUp for ProjectionRunner<P> {
    fn catch_up(&mut self) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>> {
        Box::pin(self.catch_up())
    }

    fn apply_event(&mut self, recorded: &proto::RecordedEvent) {
        apply_recorded_event(
            &mut self.checkpoint.state,
            &mut self.checkpoint.last_global_position,
            recorded,
        );
    }

    fn position(&self) -> u64 {
        self.checkpoint.last_global_position
    }

    fn save(&self) -> io::Result<()> {
        save_checkpoint::<P>(&self.checkpoint_dir, &self.checkpoint)
    }

    fn state_any(&self) -> Box<dyn Any + Send> {
        Box::new(self.checkpoint.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Empty, RecordedEvent, SubscribeResponse, subscribe_response::Content};

    /// A test projection that counts all events.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    struct EventCounter {
        pub count: u64,
    }

    impl Projection for EventCounter {
        const NAME: &'static str = "event-counter";

        fn apply(&mut self, _event: &StoredEvent) {
            self.count += 1;
        }
    }

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
    #[allow(clippy::result_large_err)]
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
    #[allow(clippy::result_large_err)]
    fn caught_up_response() -> Result<SubscribeResponse, tonic::Status> {
        Ok(SubscribeResponse {
            content: Some(Content::CaughtUp(Empty {})),
        })
    }

    #[test]
    fn projection_trait_has_no_subscriptions_method() {
        // Verify the new trait shape: NAME + apply(&mut self, &StoredEvent).
        // This test ensures the trait compiles with the expected signature.
        let mut counter = EventCounter::default();
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
        counter.apply(&stored);
        assert_eq!(counter.count, 1);
    }

    #[test]
    fn checkpoint_serializes_with_last_global_position() {
        let checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 5 },
            last_global_position: 42,
        };
        let json = serde_json::to_string(&checkpoint).expect("serialization should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(parsed["state"]["count"], 5);
        assert_eq!(parsed["last_global_position"], 42);
    }

    #[test]
    fn checkpoint_default_has_position_zero() {
        let checkpoint = ProjectionCheckpoint::<EventCounter>::default();
        assert_eq!(checkpoint.state.count, 0);
        assert_eq!(checkpoint.last_global_position, 0);
    }

    #[test]
    fn checkpoint_serde_roundtrip() {
        let checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 7 },
            last_global_position: 99,
        };
        let json = serde_json::to_string(&checkpoint).expect("serialization should succeed");
        let loaded: ProjectionCheckpoint<EventCounter> =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.last_global_position, checkpoint.last_global_position);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 7 },
            last_global_position: 99,
        };

        save_checkpoint(dir.path(), &checkpoint).expect("save should succeed");
        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(dir.path())
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.last_global_position, checkpoint.last_global_position);
    }

    #[test]
    fn load_from_empty_dir_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let result = load_checkpoint::<EventCounter>(dir.path()).expect("load should not error");
        assert!(result.is_none());
    }

    #[test]
    fn corrupt_file_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        std::fs::write(dir.path().join("checkpoint.json"), "not valid json!!!")
            .expect("write should succeed");
        let loaded = load_checkpoint::<EventCounter>(dir.path()).expect("load should not error");
        assert!(loaded.is_none());
    }

    #[test]
    fn save_creates_parent_dir() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let nested = dir.path().join("nested").join("subdir");
        let checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 3 },
            last_global_position: 10,
        };

        save_checkpoint(&nested, &checkpoint).expect("save to nested path should succeed");

        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(&nested)
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.last_global_position, checkpoint.last_global_position);
    }

    // --- process_stream / catch_up tests ---
    //
    // These tests use `tokio_stream::iter()` to mock the gRPC subscribe stream,
    // exercising the event processing logic without a live server.

    #[tokio::test]
    async fn catch_up_fresh_checkpoint_with_two_events() {
        let mut checkpoint = ProjectionCheckpoint::<EventCounter>::default();
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            event_response(1, 1),
            caught_up_response(),
        ]);

        ProjectionRunner::<EventCounter>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");

        assert_eq!(checkpoint.state.count, 2);
        // Resume token should be last_global_position + 1 = 2
        assert_eq!(checkpoint.last_global_position, 2);
    }

    #[tokio::test]
    async fn second_catch_up_with_only_caught_up_leaves_count_unchanged() {
        // Simulate a checkpoint that has already processed 2 events.
        let mut checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 2 },
            last_global_position: 2,
        };
        let stream = tokio_stream::iter(vec![caught_up_response()]);

        ProjectionRunner::<EventCounter>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");

        assert_eq!(checkpoint.state.count, 2);
        assert_eq!(checkpoint.last_global_position, 2);
    }

    #[tokio::test]
    async fn recorded_event_with_empty_metadata_is_skipped() {
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
        let mut checkpoint = ProjectionCheckpoint::<EventCounter>::default();

        ProjectionRunner::<EventCounter>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");

        // Event was skipped, count stays at 0.
        assert_eq!(checkpoint.state.count, 0);
        // But position should still advance past the skipped event.
        assert_eq!(checkpoint.last_global_position, 6);
    }

    #[tokio::test]
    async fn projection_catch_up_apply_event_decodes_and_advances_position() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint_dir = dir.path().join("event-counter");
        std::fs::create_dir_all(&checkpoint_dir).expect("create dir");

        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        let client = crate::client::EsClient::from_inner(inner);

        let mut runner = ProjectionRunner::<EventCounter>::new(client, checkpoint_dir)
            .expect("runner creation should succeed");

        let recorded = make_recorded_event(5, 0);
        let catch_up: &mut dyn ProjectionCatchUp = &mut runner;
        catch_up.apply_event(&recorded);

        assert_eq!(catch_up.position(), 6);
        // Downcast state to verify count incremented.
        let state_box = catch_up.state_any();
        let state = state_box
            .downcast::<EventCounter>()
            .expect("downcast should succeed");
        assert_eq!(state.count, 1);
    }

    #[tokio::test]
    async fn projection_catch_up_position_starts_at_zero() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint_dir = dir.path().join("event-counter");
        std::fs::create_dir_all(&checkpoint_dir).expect("create dir");

        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        let client = crate::client::EsClient::from_inner(inner);

        let runner = ProjectionRunner::<EventCounter>::new(client, checkpoint_dir)
            .expect("runner creation should succeed");

        let catch_up: &dyn ProjectionCatchUp = &runner;
        assert_eq!(catch_up.position(), 0);
    }

    #[tokio::test]
    async fn projection_catch_up_state_any_returns_cloned_state() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint_dir = dir.path().join("event-counter");
        std::fs::create_dir_all(&checkpoint_dir).expect("create dir");

        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        let client = crate::client::EsClient::from_inner(inner);

        let mut runner = ProjectionRunner::<EventCounter>::new(client, checkpoint_dir)
            .expect("runner creation should succeed");

        // Apply two events, then check state_any returns the updated state.
        let recorded = make_recorded_event(0, 0);
        let catch_up: &mut dyn ProjectionCatchUp = &mut runner;
        catch_up.apply_event(&recorded);
        let recorded = make_recorded_event(1, 1);
        catch_up.apply_event(&recorded);

        let state_box = catch_up.state_any();
        let state = state_box
            .downcast::<EventCounter>()
            .expect("downcast should succeed");
        assert_eq!(state.count, 2);
    }

    #[tokio::test]
    async fn projection_catch_up_save_persists_checkpoint() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint_dir = dir.path().join("event-counter");
        std::fs::create_dir_all(&checkpoint_dir).expect("create dir");

        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        let client = crate::client::EsClient::from_inner(inner);

        let mut runner = ProjectionRunner::<EventCounter>::new(client, checkpoint_dir.clone())
            .expect("runner creation should succeed");

        let recorded = make_recorded_event(0, 0);
        let catch_up: &mut dyn ProjectionCatchUp = &mut runner;
        catch_up.apply_event(&recorded);
        catch_up.save().expect("save should succeed");

        // Load and verify checkpoint was saved.
        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(&checkpoint_dir)
            .expect("load should succeed")
            .expect("checkpoint should exist");
        assert_eq!(loaded.state.count, 1);
        assert_eq!(loaded.last_global_position, 1);
    }

    #[test]
    fn apply_recorded_event_skips_already_processed_positions() {
        // Simulate a projection at position 5 (has processed events 0..5).
        let mut state = EventCounter::default();
        let mut position: u64 = 5;

        // Event at position 3 should be skipped (already processed).
        let old_event = make_recorded_event(3, 3);
        apply_recorded_event(&mut state, &mut position, &old_event);
        assert_eq!(state.count, 0, "should not apply already-processed event");
        assert_eq!(position, 5, "position should not change");

        // Event at position 5 should be applied (next expected position).
        let current_event = make_recorded_event(5, 5);
        apply_recorded_event(&mut state, &mut position, &current_event);
        assert_eq!(state.count, 1, "should apply event at current position");
        assert_eq!(position, 6, "position should advance");

        // Event at position 10 should also be applied (gap is fine).
        let future_event = make_recorded_event(10, 10);
        apply_recorded_event(&mut state, &mut position, &future_event);
        assert_eq!(state.count, 2, "should apply event ahead of position");
        assert_eq!(position, 11);
    }

    #[tokio::test]
    async fn checkpoint_save_load_roundtrip_via_process_stream() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint_dir = dir.path().join("event-counter");

        // Process 2 events and save checkpoint.
        let mut checkpoint = ProjectionCheckpoint::<EventCounter>::default();
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            event_response(1, 1),
            caught_up_response(),
        ]);

        ProjectionRunner::<EventCounter>::process_stream(&mut checkpoint, stream)
            .await
            .expect("process_stream should succeed");
        save_checkpoint(&checkpoint_dir, &checkpoint).expect("save should succeed");

        // Load and verify roundtrip.
        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(&checkpoint_dir)
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state.count, 2);
        assert_eq!(loaded.last_global_position, 2);
    }
}
