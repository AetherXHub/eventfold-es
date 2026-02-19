//! Process managers: cross-aggregate workflow coordination.
//!
//! A process manager reacts to events from one or more aggregate streams and
//! produces [`CommandEnvelope`]s that are dispatched to (potentially different)
//! aggregates. They are structurally similar to projections -- they use
//! checkpointed cursors for catch-up -- but produce side effects (commands)
//! rather than read models.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::command::CommandEnvelope;
use crate::error::DispatchError;
use crate::projection::CursorPosition;
use crate::storage::StreamLayout;

/// A cross-aggregate workflow coordinator that reacts to events by producing
/// commands.
///
/// Process managers subscribe to aggregate event streams (like projections)
/// but instead of building a read model, they emit [`CommandEnvelope`]s that
/// the store dispatches to target aggregates.
///
/// # Contract
///
/// - [`react`](ProcessManager::react) must be deterministic: given the same
///   sequence of events, it must produce the same command envelopes.
/// - Unknown event types should be silently ignored for forward compatibility.
/// - State is checkpointed after all envelopes from a catch-up pass have been
///   dispatched (or dead-lettered), ensuring crash safety via re-processing.
pub trait ProcessManager: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Human-readable name, used as a directory name under `process_managers/`.
    const NAME: &'static str;

    /// The aggregate types this process manager subscribes to.
    ///
    /// Each entry must match an `Aggregate::AGGREGATE_TYPE` string.
    fn subscriptions(&self) -> &'static [&'static str];

    /// React to a single event from a subscribed stream.
    ///
    /// Returns zero or more [`CommandEnvelope`]s to dispatch.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - Which aggregate produced the event.
    /// * `stream_id` - The specific aggregate instance.
    /// * `event` - The raw eventfold event.
    fn react(
        &mut self,
        aggregate_type: &str,
        stream_id: &str,
        event: &eventfold::Event,
    ) -> Vec<CommandEnvelope>;
}

/// Persisted state of a process manager including per-stream cursors.
///
/// Structurally identical to `ProjectionCheckpoint`, but kept separate to
/// avoid coupling the two subsystems. The `cursors` map uses the same
/// `"aggregate_type/instance_id"` key encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessManagerCheckpoint<PM> {
    /// The process manager's current state.
    state: PM,
    /// Per-stream cursor positions, keyed by (aggregate_type, instance_id).
    #[serde(with = "cursor_map")]
    cursors: HashMap<(String, String), CursorPosition>,
}

impl<PM: Default> Default for ProcessManagerCheckpoint<PM> {
    fn default() -> Self {
        Self {
            state: PM::default(),
            cursors: HashMap::new(),
        }
    }
}

/// Custom serde for `HashMap<(String, String), CursorPosition>`.
///
/// Mirrors the identical module in `projection.rs`. JSON object keys must be
/// strings, so we encode each `(aggregate_type, instance_id)` tuple as
/// `"aggregate_type/instance_id"`.
mod cursor_map {
    use super::*;
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};

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

// --- Checkpoint persistence helpers ---

/// Save a process manager checkpoint atomically.
///
/// Writes to a temporary file then renames to `checkpoint.json` in `dir`.
/// Creates `dir` if it does not exist.
fn save_pm_checkpoint<PM: ProcessManager>(
    dir: &std::path::Path,
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
fn load_pm_checkpoint<PM: ProcessManager>(
    dir: &std::path::Path,
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

/// Delete the checkpoint file if it exists.
#[allow(dead_code)] // Used by ProcessManagerRunner::rebuild, which is test-only.
fn delete_pm_checkpoint(dir: &std::path::Path) -> io::Result<()> {
    let path = dir.join("checkpoint.json");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// --- ProcessManagerRunner ---

/// Drives a process manager's catch-up loop, reading events from subscribed
/// streams and collecting command envelopes for dispatch.
///
/// Unlike [`ProjectionRunner`](crate::projection::ProjectionRunner), `catch_up`
/// does **not** persist the checkpoint. Instead it returns the collected
/// envelopes, and the caller must invoke [`save`](ProcessManagerRunner::save)
/// after all envelopes have been dispatched (or dead-lettered). This ensures
/// crash safety: a crash mid-dispatch causes re-processing on restart.
pub(crate) struct ProcessManagerRunner<PM: ProcessManager> {
    checkpoint: ProcessManagerCheckpoint<PM>,
    layout: StreamLayout,
    checkpoint_dir: PathBuf,
}

impl<PM: ProcessManager> ProcessManagerRunner<PM> {
    /// Create a new runner, loading an existing checkpoint from disk if
    /// available.
    ///
    /// # Arguments
    ///
    /// * `layout` - The shared [`StreamLayout`] for resolving stream paths.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if reading an existing checkpoint file fails
    /// (other than file-not-found).
    pub(crate) fn new(layout: StreamLayout) -> io::Result<Self> {
        let checkpoint_dir = layout.process_managers_dir().join(PM::NAME);
        let checkpoint = load_pm_checkpoint::<PM>(&checkpoint_dir)?.unwrap_or_default();
        Ok(Self {
            checkpoint,
            layout,
            checkpoint_dir,
        })
    }

    /// Catch up on all subscribed streams, returning command envelopes.
    ///
    /// Reads new events from each subscribed stream, invokes
    /// [`ProcessManager::react`] for each event, and collects the resulting
    /// envelopes. Updates in-memory cursors but does **not** persist the
    /// checkpoint.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if listing streams or reading events fails.
    pub(crate) fn catch_up(&mut self) -> io::Result<Vec<CommandEnvelope>> {
        let _span = tracing::debug_span!("pm_catchup", pm_name = PM::NAME,).entered();

        let mut envelopes = Vec::new();

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
                // Skip streams with no log file yet.
                let iter = match reader.read_from(offset) {
                    Ok(iter) => iter,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                for result in iter {
                    let (event, next_offset, line_hash) = result?;
                    let produced = self
                        .checkpoint
                        .state
                        .react(aggregate_type, instance_id, &event);
                    envelopes.extend(produced);
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

    /// Returns the current process manager state.
    #[allow(dead_code)] // Used in integration tests.
    pub(crate) fn state(&self) -> &PM {
        &self.checkpoint.state
    }

    /// Delete checkpoint and replay all events from scratch.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if deleting the checkpoint or catching up fails.
    #[allow(dead_code)] // Used in integration tests.
    pub(crate) fn rebuild(&mut self) -> io::Result<Vec<CommandEnvelope>> {
        delete_pm_checkpoint(&self.checkpoint_dir)?;
        self.checkpoint = ProcessManagerCheckpoint::default();
        self.catch_up()
    }

    /// Returns the path to this process manager's dead-letter log.
    pub(crate) fn dead_letter_path(&self) -> PathBuf {
        self.checkpoint_dir.join("dead_letters.jsonl")
    }

    /// Returns the process manager name.
    #[allow(dead_code)] // Part of the ProcessManagerCatchUp interface.
    pub(crate) fn name(&self) -> &str {
        PM::NAME
    }
}

// --- Type-erased trait for store integration ---

/// Trait object interface for process manager runners.
///
/// Allows `run_process_managers` to iterate over heterogeneous process
/// managers without knowing each concrete `PM` type.
pub(crate) trait ProcessManagerCatchUp: Send + Sync {
    /// Catch up on subscribed streams and return command envelopes.
    fn catch_up(&mut self) -> io::Result<Vec<CommandEnvelope>>;

    /// Persist the checkpoint to disk.
    fn save(&self) -> io::Result<()>;

    /// Returns the process manager name.
    #[allow(dead_code)] // Not yet consumed outside tests.
    fn name(&self) -> &str;

    /// Returns the path to the dead-letter log file.
    fn dead_letter_path(&self) -> PathBuf;
}

impl<PM: ProcessManager> ProcessManagerCatchUp for ProcessManagerRunner<PM> {
    fn catch_up(&mut self) -> io::Result<Vec<CommandEnvelope>> {
        self.catch_up()
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

// --- Dispatch infrastructure ---

/// Type-erased dispatcher for a single aggregate type.
///
/// Each registered aggregate type gets a `TypedDispatcher<A>` that knows
/// how to deserialize the command JSON and route it through the store.
pub(crate) trait AggregateDispatcher: Send + Sync {
    /// Dispatch a command envelope to the target aggregate.
    ///
    /// # Arguments
    ///
    /// * `store` - The aggregate store for looking up aggregate handles.
    /// * `envelope` - The command envelope to dispatch.
    fn dispatch<'a>(
        &'a self,
        store: &'a crate::store::AggregateStore,
        envelope: CommandEnvelope,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), DispatchError>> + Send + 'a>>;
}

/// Concrete dispatcher for aggregate type `A`.
///
/// Deserializes `CommandEnvelope.command` into `A::Command`, then calls
/// `store.get::<A>().execute()`.
pub(crate) struct TypedDispatcher<A> {
    // PhantomData to carry the aggregate type without storing a value.
    _marker: std::marker::PhantomData<A>,
}

impl<A> TypedDispatcher<A> {
    pub(crate) fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<A> AggregateDispatcher for TypedDispatcher<A>
where
    A: crate::aggregate::Aggregate,
    A::Command: DeserializeOwned,
{
    fn dispatch<'a>(
        &'a self,
        store: &'a crate::store::AggregateStore,
        envelope: CommandEnvelope,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), DispatchError>> + Send + 'a>>
    {
        Box::pin(async move {
            // Deserialize the type-erased JSON into the concrete command type.
            let cmd: A::Command =
                serde_json::from_value(envelope.command).map_err(DispatchError::Deserialization)?;

            let handle = store
                .get::<A>(&envelope.instance_id)
                .await
                .map_err(DispatchError::Io)?;

            handle
                .execute(cmd, envelope.context)
                .await
                .map_err(|e| DispatchError::Execution(Box::new(e)))?;

            Ok(())
        })
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
    path: &std::path::Path,
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

        fn subscriptions(&self) -> &'static [&'static str] {
            &["counter"]
        }

        fn react(
            &mut self,
            _aggregate_type: &str,
            stream_id: &str,
            event: &eventfold::Event,
        ) -> Vec<CommandEnvelope> {
            self.events_seen += 1;
            // Echo each event as a command to a "target" aggregate,
            // using the source stream_id as the target instance_id.
            vec![CommandEnvelope {
                aggregate_type: "target".to_string(),
                instance_id: stream_id.to_string(),
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

    use crate::aggregate::test_fixtures::{Counter, CounterCommand};
    use crate::command::CommandContext;
    use crate::store::AggregateStore;

    fn dummy_event() -> eventfold::Event {
        eventfold::Event::new("Incremented", serde_json::json!(null))
    }

    #[test]
    fn echo_saga_react_produces_envelope() {
        let mut saga = EchoSaga::default();
        let event = dummy_event();
        let envelopes = saga.react("counter", "c-1", &event);

        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].aggregate_type, "target");
        assert_eq!(envelopes[0].instance_id, "c-1");
        assert_eq!(envelopes[0].command["source_event_type"], "Incremented");
        assert_eq!(saga.events_seen, 1);
    }

    #[test]
    fn echo_saga_subscriptions() {
        let saga = EchoSaga::default();
        assert_eq!(saga.subscriptions(), &["counter"]);
    }

    #[test]
    fn checkpoint_serde_roundtrip() {
        let mut checkpoint = ProcessManagerCheckpoint {
            state: EchoSaga { events_seen: 3 },
            cursors: HashMap::new(),
        };
        checkpoint.cursors.insert(
            ("counter".to_string(), "c-1".to_string()),
            CursorPosition {
                offset: 256,
                hash: "abc123".to_string(),
            },
        );

        let json = serde_json::to_string(&checkpoint).expect("serialization should succeed");
        let deserialized: ProcessManagerCheckpoint<EchoSaga> =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.state, checkpoint.state);
        assert_eq!(deserialized.cursors, checkpoint.cursors);
    }

    // --- ProcessManagerRunner integration tests ---

    /// Helper: execute a single `Increment` command on the given instance.
    async fn increment(store: &AggregateStore, id: &str) {
        let handle = store.get::<Counter>(id).await.expect("get should succeed");
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment should succeed");
    }

    #[tokio::test]
    async fn catch_up_produces_envelopes() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;

        let mut runner = ProcessManagerRunner::<EchoSaga>::new(store.layout().clone())
            .expect("runner creation should succeed");
        let envelopes = runner.catch_up().expect("catch_up should succeed");

        assert_eq!(envelopes.len(), 2);
        assert_eq!(runner.state().events_seen, 2);
    }

    #[tokio::test]
    async fn cursors_advance_no_re_emit() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;

        let mut runner = ProcessManagerRunner::<EchoSaga>::new(store.layout().clone())
            .expect("runner creation should succeed");
        let first = runner.catch_up().expect("first catch_up should succeed");
        assert_eq!(first.len(), 1);
        runner.save().expect("save should succeed");

        // Second catch_up with no new events should produce nothing.
        let second = runner.catch_up().expect("second catch_up should succeed");
        assert!(second.is_empty());
    }

    #[tokio::test]
    async fn checkpoint_persists_and_restores() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-1").await;

        // First runner: catch up and save.
        {
            let mut runner = ProcessManagerRunner::<EchoSaga>::new(store.layout().clone())
                .expect("runner creation should succeed");
            let envelopes = runner.catch_up().expect("catch_up should succeed");
            assert_eq!(envelopes.len(), 2);
            runner.save().expect("save should succeed");
        }

        // Second runner: should load checkpoint, no re-emit.
        let mut runner = ProcessManagerRunner::<EchoSaga>::new(store.layout().clone())
            .expect("runner reload should succeed");
        assert_eq!(runner.state().events_seen, 2);
        let envelopes = runner.catch_up().expect("catch_up should succeed");
        assert!(envelopes.is_empty());
    }

    #[tokio::test]
    async fn rebuild_replays_all_events() {
        let tmp = tempfile::tempdir().expect("failed to create tmpdir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;
        increment(&store, "c-2").await;

        let mut runner = ProcessManagerRunner::<EchoSaga>::new(store.layout().clone())
            .expect("runner creation should succeed");
        let first = runner.catch_up().expect("catch_up should succeed");
        assert_eq!(first.len(), 3);
        runner.save().expect("save should succeed");

        let rebuilt = runner.rebuild().expect("rebuild should succeed");
        assert_eq!(rebuilt.len(), 3);
        assert_eq!(runner.state().events_seen, 3);
    }

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
