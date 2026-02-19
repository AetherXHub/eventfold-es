//! Cross-stream projections (read models) that fold events from multiple aggregate streams.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A cross-stream read model that consumes events from multiple aggregate streams.
///
/// Projections are eventually consistent: they catch up by reading new events
/// from subscribed streams and are rebuilt from scratch if their checkpoint
/// is invalid.
///
/// # Contract
///
/// - [`apply`](Projection::apply) must be deterministic: given the same sequence
///   of events, it must produce the same state.
/// - Unknown event types should be silently ignored for forward compatibility.
pub trait Projection:
    Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static
{
    /// Human-readable name, used as a directory name under `projections/`.
    const NAME: &'static str;

    /// The aggregate types this projection subscribes to.
    ///
    /// Each entry must match an `Aggregate::AGGREGATE_TYPE` string.
    fn subscriptions(&self) -> &'static [&'static str];

    /// Apply a single event from any subscribed stream.
    ///
    /// `aggregate_type` identifies which aggregate produced the event.
    /// `stream_id` identifies the specific aggregate instance.
    fn apply(&mut self, aggregate_type: &str, stream_id: &str, event: &eventfold::Event);
}

/// Tracks the read position within a single event stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CursorPosition {
    /// Byte offset past the last consumed event.
    pub offset: u64,
    /// Hash of the last consumed event line.
    pub hash: String,
}

/// Persisted state of a projection including per-stream cursors.
///
/// The `cursors` map is keyed by `(aggregate_type, instance_id)` tuples,
/// allowing the projection to track independent read positions across
/// multiple aggregate streams. A custom serde module encodes each tuple
/// key as `"aggregate_type/instance_id"` so the map serializes to valid JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProjectionCheckpoint<P> {
    /// The projection's current state.
    pub state: P,
    /// Per-stream cursor positions, keyed by (aggregate_type, instance_id).
    #[serde(with = "cursor_map")]
    pub cursors: HashMap<(String, String), CursorPosition>,
}

/// Custom serde for `HashMap<(String, String), CursorPosition>`.
///
/// JSON object keys must be strings, so we encode each `(aggregate_type,
/// instance_id)` tuple as `"aggregate_type/instance_id"`. The separator
/// `/` is safe because aggregate types and instance IDs are path-safe
/// identifiers that never contain `/`.
mod cursor_map {
    use super::*;
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};

    /// Separator between aggregate_type and instance_id in the serialized key.
    const SEP: char = '/';

    pub fn serialize<S>(
        map: &HashMap<(String, String), CursorPosition>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut ser_map = serializer.serialize_map(Some(map.len()))?;
        for ((agg, id), cursor) in map {
            let key = format!("{agg}{SEP}{id}");
            ser_map.serialize_entry(&key, cursor)?;
        }
        ser_map.end()
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<(String, String), CursorPosition>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize as a flat string-keyed map first, then split keys.
        let raw: HashMap<String, CursorPosition> = HashMap::deserialize(deserializer)?;
        raw.into_iter()
            .map(|(key, cursor)| {
                let (agg, id) = key.split_once(SEP).ok_or_else(|| {
                    serde::de::Error::custom(format!("cursor key missing '{SEP}' separator: {key}"))
                })?;
                Ok(((agg.to_string(), id.to_string()), cursor))
            })
            .collect()
    }
}

impl<P: Default> Default for ProjectionCheckpoint<P> {
    fn default() -> Self {
        Self {
            state: P::default(),
            cursors: HashMap::new(),
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

/// Delete the checkpoint file if it exists.
///
/// # Arguments
///
/// * `dir` - Directory containing the `checkpoint.json` file to remove.
///
/// # Errors
///
/// Returns `io::Error` for I/O failures other than file-not-found.
#[allow(dead_code)] // Used by ProjectionRunner::rebuild, which is test-only.
pub(crate) fn delete_checkpoint(dir: &Path) -> io::Result<()> {
    let path = dir.join("checkpoint.json");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Drives a projection's catch-up loop, reading events from subscribed streams.
///
/// Manages the lifecycle of a single [`Projection`]: loading its persisted
/// checkpoint, catching up on new events from all subscribed aggregate
/// streams, and saving the checkpoint back to disk.
pub(crate) struct ProjectionRunner<P: Projection> {
    /// The projection's persisted state and per-stream cursor positions.
    checkpoint: ProjectionCheckpoint<P>,
    /// Shared directory layout for locating stream directories.
    layout: crate::storage::StreamLayout,
    /// Directory where this projection's checkpoint file is stored.
    checkpoint_dir: PathBuf,
}

impl<P: Projection> ProjectionRunner<P> {
    /// Create a new runner, loading an existing checkpoint from disk if available.
    ///
    /// If no checkpoint file exists, the runner starts with a default
    /// (empty) projection state and no cursor positions.
    ///
    /// # Arguments
    ///
    /// * `layout` - The shared [`StreamLayout`] for resolving stream paths.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if reading an existing checkpoint file fails
    /// (other than file-not-found).
    pub(crate) fn new(layout: crate::storage::StreamLayout) -> io::Result<Self> {
        let checkpoint_dir = layout.projections_dir().join(P::NAME);
        let checkpoint = load_checkpoint::<P>(&checkpoint_dir)?.unwrap_or_default();
        Ok(Self {
            checkpoint,
            layout,
            checkpoint_dir,
        })
    }

    /// Catch up on all subscribed streams.
    ///
    /// Discovers streams for each subscribed aggregate type, reads new events
    /// from each stream starting at the last known cursor offset, applies them
    /// to the projection state, and saves the updated checkpoint to disk.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if listing streams, reading events, or saving
    /// the checkpoint fails.
    pub(crate) fn catch_up(&mut self) -> io::Result<()> {
        for &aggregate_type in self.checkpoint.state.subscriptions() {
            let instance_ids = self.layout.list_streams(aggregate_type)?;
            for instance_id in &instance_ids {
                let stream_dir = self.layout.stream_dir(aggregate_type, instance_id);
                let reader = eventfold::EventReader::new(&stream_dir);
                let key = (aggregate_type.to_owned(), instance_id.clone());
                let offset = self
                    .checkpoint
                    .cursors
                    .get(&key)
                    .map(|c| c.offset)
                    .unwrap_or(0);
                // Skip streams with no log file yet (the directory exists
                // but app.jsonl hasn't been created). This avoids erroring
                // on newly-created but unwritten streams.
                let iter = match reader.read_from(offset) {
                    Ok(iter) => iter,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                for result in iter {
                    let (event, next_offset, line_hash) = result?;
                    self.checkpoint
                        .state
                        .apply(aggregate_type, instance_id, &event);
                    self.checkpoint.cursors.insert(
                        key.clone(),
                        CursorPosition {
                            offset: next_offset,
                            hash: line_hash,
                        },
                    );
                }
            }
        }
        save_checkpoint::<P>(&self.checkpoint_dir, &self.checkpoint)?;
        Ok(())
    }

    /// Returns the current projection state.
    pub(crate) fn state(&self) -> &P {
        &self.checkpoint.state
    }

    /// Delete checkpoint and replay all events from scratch.
    ///
    /// Removes the persisted checkpoint file, resets internal state to
    /// defaults, then performs a full catch-up from offset zero on all
    /// subscribed streams.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if deleting the checkpoint or catching up fails.
    #[allow(dead_code)] // Used in integration tests.
    pub(crate) fn rebuild(&mut self) -> io::Result<()> {
        delete_checkpoint(&self.checkpoint_dir)?;
        self.checkpoint = ProjectionCheckpoint::default();
        self.catch_up()
    }
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;

    /// A test projection that counts all events across subscribed streams.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub(crate) struct EventCounter {
        pub count: u64,
    }

    impl Projection for EventCounter {
        const NAME: &'static str = "event-counter";

        fn subscriptions(&self) -> &'static [&'static str] {
            &["counter"]
        }

        fn apply(&mut self, _aggregate_type: &str, _stream_id: &str, _event: &eventfold::Event) {
            self.count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::EventCounter;

    /// Helper to create a dummy event for testing.
    fn dummy_event() -> eventfold::Event {
        eventfold::Event::new("test", serde_json::json!({}))
    }

    #[test]
    fn apply_increments_count() {
        let mut counter = EventCounter::default();
        let event = dummy_event();

        counter.apply("counter", "instance-1", &event);
        counter.apply("counter", "instance-1", &event);
        counter.apply("counter", "instance-2", &event);

        assert_eq!(counter.count, 3);
    }

    #[test]
    fn subscriptions_returns_expected() {
        let counter = EventCounter::default();
        assert_eq!(counter.subscriptions(), &["counter"]);
    }

    #[test]
    fn checkpoint_serialization_roundtrip() {
        let mut checkpoint = ProjectionCheckpoint {
            state: EventCounter { count: 5 },
            cursors: HashMap::new(),
        };
        checkpoint.cursors.insert(
            ("counter".to_string(), "inst-1".to_string()),
            CursorPosition {
                offset: 128,
                hash: "abc123".to_string(),
            },
        );

        let json = serde_json::to_string(&checkpoint).expect("serialization should succeed");
        let deserialized: ProjectionCheckpoint<EventCounter> =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.state, checkpoint.state);
        assert_eq!(deserialized.cursors, checkpoint.cursors);
    }

    #[test]
    fn checkpoint_default_has_empty_cursors() {
        let checkpoint = ProjectionCheckpoint::<EventCounter>::default();
        assert_eq!(checkpoint.state.count, 0);
        assert!(checkpoint.cursors.is_empty());
    }

    /// Helper to build a checkpoint with a single cursor entry for persistence tests.
    fn sample_checkpoint() -> ProjectionCheckpoint<EventCounter> {
        let mut cursors = HashMap::new();
        cursors.insert(
            ("counter".to_string(), "inst-1".to_string()),
            CursorPosition {
                offset: 256,
                hash: "deadbeef".to_string(),
            },
        );
        ProjectionCheckpoint {
            state: EventCounter { count: 7 },
            cursors,
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint = sample_checkpoint();

        save_checkpoint(dir.path(), &checkpoint).expect("save should succeed");
        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(dir.path())
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.cursors, checkpoint.cursors);
    }

    #[test]
    fn load_from_empty_dir_returns_none() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");

        let result = load_checkpoint::<EventCounter>(dir.path()).expect("load should not error");

        assert!(result.is_none());
    }

    #[test]
    fn delete_removes_file() {
        let dir = tempfile::tempdir().expect("failed to create tmpdir");
        let checkpoint = sample_checkpoint();

        save_checkpoint(dir.path(), &checkpoint).expect("save should succeed");
        delete_checkpoint(dir.path()).expect("delete should succeed");

        let loaded = load_checkpoint::<EventCounter>(dir.path()).expect("load should not error");
        assert!(loaded.is_none());
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
        let checkpoint = sample_checkpoint();

        save_checkpoint(&nested, &checkpoint).expect("save to nested path should succeed");

        let loaded: ProjectionCheckpoint<EventCounter> = load_checkpoint(&nested)
            .expect("load should succeed")
            .expect("checkpoint should exist");

        assert_eq!(loaded.state, checkpoint.state);
        assert_eq!(loaded.cursors, checkpoint.cursors);
    }

    // --- ProjectionRunner integration tests ---
    //
    // These tests use `AggregateStore` to write real events on disk and then
    // verify that `ProjectionRunner` correctly reads and applies them.

    use crate::aggregate::test_fixtures::{Counter, CounterCommand};
    use crate::command::CommandContext;
    use crate::store::AggregateStore;

    /// Helper: execute a single `Increment` command on the given instance.
    async fn increment(store: &AggregateStore, id: &str) {
        let handle = store.get::<Counter>(id).await.expect("get should succeed");
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment should succeed");
    }

    #[tokio::test]
    async fn catch_up_processes_all_events() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;

        let mut runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
            .expect("runner creation should succeed");
        runner.catch_up().expect("catch_up should succeed");

        assert_eq!(runner.state().count, 2);
    }

    #[tokio::test]
    async fn incremental_catch_up() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-1").await;

        let mut runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
            .expect("runner creation should succeed");
        runner.catch_up().expect("first catch_up should succeed");
        assert_eq!(runner.state().count, 2);

        // Append 2 more events to the same stream.
        increment(&store, "c-1").await;
        increment(&store, "c-1").await;

        runner.catch_up().expect("second catch_up should succeed");
        // Should be 4 (not 6) -- the first 2 events are not re-processed.
        assert_eq!(runner.state().count, 4);
    }

    #[tokio::test]
    async fn new_stream_discovery() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;

        let mut runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
            .expect("runner creation should succeed");
        runner.catch_up().expect("first catch_up should succeed");
        assert_eq!(runner.state().count, 1);

        // Create a brand-new stream and append an event.
        increment(&store, "c-2").await;

        runner.catch_up().expect("second catch_up should succeed");
        assert_eq!(runner.state().count, 2);
    }

    #[tokio::test]
    async fn rebuild_replays_everything() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-1").await;
        increment(&store, "c-2").await;

        let mut runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
            .expect("runner creation should succeed");
        runner.catch_up().expect("catch_up should succeed");
        assert_eq!(runner.state().count, 3);

        runner.rebuild().expect("rebuild should succeed");
        // Same count -- events replayed from scratch.
        assert_eq!(runner.state().count, 3);
    }

    #[tokio::test]
    async fn checkpoint_persists_across_runner_restart() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;

        // First runner: catch up and save checkpoint.
        {
            let mut runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
                .expect("runner creation should succeed");
            runner.catch_up().expect("catch_up should succeed");
            assert_eq!(runner.state().count, 2);
        }
        // Runner dropped -- checkpoint was saved by catch_up().

        // Second runner: should load the checkpoint from disk.
        let runner = ProjectionRunner::<EventCounter>::new(store.layout().clone())
            .expect("runner reload should succeed");
        // State should reflect the persisted checkpoint without calling catch_up.
        assert_eq!(runner.state().count, 2);
    }
}
