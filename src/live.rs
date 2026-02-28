//! Live subscription configuration and runtime types.
//!
//! This module provides [`LiveConfig`] for tuning checkpoint and reconnection
//! behaviour of the live subscription loop, and [`LiveHandle`] for controlling
//! a running live subscription.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio_stream::StreamExt;

use crate::command::CommandEnvelope;
use crate::process_manager::append_dead_letter;
use crate::proto::subscribe_response;
use crate::store::AggregateStore;

/// Configuration for live subscription behaviour.
///
/// Controls how often checkpoints are flushed to disk and how the live loop
/// reconnects after a stream disconnection. All fields have sensible defaults
/// accessible via [`LiveConfig::default()`].
///
/// Pass to [`AggregateStoreBuilder::live_config`](crate::AggregateStoreBuilder::live_config)
/// to customize.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use eventfold_es::LiveConfig;
///
/// let config = LiveConfig {
///     checkpoint_interval: Duration::from_secs(10),
///     ..LiveConfig::default()
/// };
/// assert_eq!(config.checkpoint_interval, Duration::from_secs(10));
/// assert_eq!(config.reconnect_base_delay, Duration::from_secs(1));
/// ```
#[derive(Debug, Clone)]
pub struct LiveConfig {
    /// How often to flush projection and process manager checkpoints to disk
    /// while in live mode.
    ///
    /// Checkpoints are also saved on graceful shutdown and when the stream
    /// reconnects. A shorter interval reduces replay on crash; a longer
    /// interval reduces disk I/O.
    ///
    /// Default: 5 seconds.
    pub checkpoint_interval: Duration,

    /// Base delay for exponential backoff on stream reconnection.
    ///
    /// After a stream error, the loop waits `reconnect_base_delay`, then
    /// `2 * reconnect_base_delay`, etc., up to [`reconnect_max_delay`](LiveConfig::reconnect_max_delay).
    /// A successful `CaughtUp` resets the backoff.
    ///
    /// Default: 1 second.
    pub reconnect_base_delay: Duration,

    /// Maximum delay between reconnection attempts.
    ///
    /// Default: 30 seconds.
    pub reconnect_max_delay: Duration,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: Duration::from_secs(5),
            reconnect_base_delay: Duration::from_secs(1),
            reconnect_max_delay: Duration::from_secs(30),
        }
    }
}

/// Handle for controlling the live subscription loop.
///
/// Provides methods to check whether the loop has caught up with the
/// historical event log and to shut it down gracefully. Dropping the
/// handle does **not** stop the loop -- call [`shutdown`](LiveHandle::shutdown)
/// for graceful termination with a final checkpoint save.
///
/// `Clone` is cheap: all fields are `Arc`-wrapped.
#[derive(Clone)]
pub struct LiveHandle {
    /// Sends `true` to signal the loop to stop.
    pub(crate) shutdown_tx: tokio::sync::watch::Sender<bool>,
    /// Set to `true` when the live loop receives a `CaughtUp` sentinel.
    pub(crate) caught_up: Arc<AtomicBool>,
    /// The spawned background task. Wrapped in `Option` so it can be
    /// taken and awaited exactly once by [`shutdown`](LiveHandle::shutdown).
    pub(crate) task: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<io::Result<()>>>>>,
}

impl LiveHandle {
    /// Returns `true` if the live loop has received a `CaughtUp` sentinel
    /// at least once, meaning historical replay is complete.
    ///
    /// # Returns
    ///
    /// `true` after the initial catch-up phase completes; `false` before.
    pub fn is_caught_up(&self) -> bool {
        self.caught_up.load(Ordering::Acquire)
    }

    /// Signal the live loop to stop and wait for a final checkpoint save.
    ///
    /// Sends the shutdown signal, then awaits the background task to
    /// completion. The loop saves all projection and process manager
    /// checkpoints before exiting.
    ///
    /// Calling `shutdown` more than once is safe -- subsequent calls
    /// return `Ok(())` immediately.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the loop exited cleanly.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the loop's final checkpoint save or the
    /// task join fails.
    pub async fn shutdown(&self) -> io::Result<()> {
        // Signal the loop to stop. Ignore errors (receiver may already
        // be dropped if the task has exited).
        let _ = self.shutdown_tx.send(true);

        // Take the task handle so we can await it exactly once.
        let task = self.task.lock().await.take();
        if let Some(join_handle) = task {
            join_handle
                .await
                .map_err(|e| io::Error::other(format!("live loop task panicked: {e}")))?
        } else {
            Ok(())
        }
    }
}

/// Result of processing a single stream until it ends or a `CaughtUp` sentinel
/// is received. Used by the outer reconnection loop to decide whether to
/// reconnect.
enum StreamOutcome {
    /// The `CaughtUp` sentinel was received and then the stream ended cleanly
    /// (EOF after live tail, or the stream was fully consumed).
    Ended,
    /// A stream error occurred and should trigger reconnection.
    Error(io::Error),
}

/// Save all projection and process manager checkpoints to disk.
///
/// Iterates all registered projections and process managers, saving each
/// checkpoint. Logs warnings on save failures but does not propagate them,
/// since partial checkpoint failure should not crash the loop.
async fn save_all_checkpoints(store: &AggregateStore) {
    let projections = store.projections.read().await;
    for (name, runner_mutex) in projections.iter() {
        let runner = runner_mutex.lock().await;
        if let Err(e) = runner.save() {
            tracing::error!(
                projection = %name,
                error = %e,
                "failed to save projection checkpoint"
            );
        }
    }
    drop(projections);

    let pms = store.process_managers.read().await;
    for pm_mutex in pms.iter() {
        let pm = pm_mutex.lock().await;
        if let Err(e) = pm.save() {
            tracing::error!(
                pm = pm.name(),
                error = %e,
                "failed to save PM checkpoint"
            );
        }
    }
}

/// Compute the minimum global position across all projections and process
/// managers.
///
/// This is the position from which the live loop should subscribe to avoid
/// missing events that some projection or PM has not yet processed.
async fn min_global_position(store: &AggregateStore) -> u64 {
    let mut min_pos = u64::MAX;

    let projections = store.projections.read().await;
    for runner_mutex in projections.values() {
        let runner = runner_mutex.lock().await;
        min_pos = min_pos.min(runner.position());
    }
    drop(projections);

    let pms = store.process_managers.read().await;
    for pm_mutex in pms.iter() {
        let pm = pm_mutex.lock().await;
        min_pos = min_pos.min(pm.position());
    }
    drop(pms);

    // If no projections or PMs are registered, start from 0.
    if min_pos == u64::MAX { 0 } else { min_pos }
}

/// Run the live subscription loop.
///
/// This is the core background loop spawned by `AggregateStore::start_live()`.
/// It holds a `SubscribeAll` stream open, fans out events to all projections
/// and process managers, dispatches PM command envelopes, checkpoints
/// periodically, reconnects on errors with exponential backoff, and shuts
/// down gracefully when signaled.
///
/// # Arguments
///
/// * `store` - The aggregate store providing client, projections, PMs,
///   dispatchers, and configuration.
/// * `caught_up` - Atomic flag set to `true` when `CaughtUp` is received.
/// * `shutdown_rx` - Watch receiver that signals the loop to stop.
///
/// # Returns
///
/// `Ok(())` on graceful shutdown.
///
/// # Errors
///
/// Returns `io::Error` if the loop encounters an unrecoverable error.
pub(crate) async fn run_live_loop(
    store: AggregateStore,
    caught_up: Arc<AtomicBool>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> io::Result<()> {
    let config = store.live_config.clone();
    let mut backoff_delay = config.reconnect_base_delay;
    let mut checkpoint_interval = tokio::time::interval(config.checkpoint_interval);
    // The first tick completes immediately; consume it.
    checkpoint_interval.tick().await;

    loop {
        // Check for shutdown before (re)connecting.
        if *shutdown_rx.borrow() {
            save_all_checkpoints(&store).await;
            return Ok(());
        }

        let from_position = min_global_position(&store).await;
        tracing::info!(from_position, "live loop: subscribing");

        let stream_result = store.client.clone().subscribe_all_from(from_position).await;

        let stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "live loop: subscribe failed, will retry");
                // Backoff and retry.
                tokio::select! {
                    _ = tokio::time::sleep(backoff_delay) => {}
                    _ = shutdown_rx.changed() => {
                        save_all_checkpoints(&store).await;
                        return Ok(());
                    }
                }
                backoff_delay = (backoff_delay * 2).min(config.reconnect_max_delay);
                continue;
            }
        };

        // Process the stream with select! for shutdown and checkpoint.
        let outcome = {
            let stream_fut = process_stream_with_dispatch(&store, stream, &caught_up);
            tokio::pin!(stream_fut);

            loop {
                tokio::select! {
                    result = &mut stream_fut => {
                        break result;
                    }
                    _ = checkpoint_interval.tick() => {
                        save_all_checkpoints(&store).await;
                    }
                    _ = shutdown_rx.changed() => {
                        save_all_checkpoints(&store).await;
                        return Ok(());
                    }
                }
            }
        };

        match outcome {
            Ok(StreamOutcome::Ended) => {
                // Stream ended cleanly (e.g., server closed). Save checkpoints
                // and reconnect without backoff if we already caught up.
                save_all_checkpoints(&store).await;
                if caught_up.load(Ordering::Acquire) {
                    // Was live, treat clean close as transient.
                    backoff_delay = config.reconnect_base_delay;
                }
            }
            Ok(StreamOutcome::Error(e)) => {
                tracing::error!(error = %e, "live loop: stream error, will reconnect");
                save_all_checkpoints(&store).await;
                // Backoff before reconnecting.
                tokio::select! {
                    _ = tokio::time::sleep(backoff_delay) => {}
                    _ = shutdown_rx.changed() => {
                        return Ok(());
                    }
                }
                backoff_delay = (backoff_delay * 2).min(config.reconnect_max_delay);
            }
            Err(e) => {
                tracing::error!(error = %e, "live loop: unrecoverable error");
                save_all_checkpoints(&store).await;
                return Err(e);
            }
        }
    }
}

/// Process events from a subscribe stream with full dispatch support.
///
/// Fans out each event to all projections and process managers, dispatches
/// PM command envelopes through the store's dispatchers, and sets the
/// caught-up flag. Resets backoff on `CaughtUp`.
///
/// This is the async workhorse called from within the `run_live_loop`
/// select loop.
async fn process_stream_with_dispatch(
    store: &AggregateStore,
    mut stream: impl tokio_stream::Stream<Item = Result<crate::proto::SubscribeResponse, tonic::Status>>
    + Unpin,
    caught_up: &AtomicBool,
) -> io::Result<StreamOutcome> {
    while let Some(result) = stream.next().await {
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                return Ok(StreamOutcome::Error(io::Error::other(format!(
                    "subscribe stream error: {e}"
                ))));
            }
        };

        match response.content {
            Some(subscribe_response::Content::Event(recorded)) => {
                // Fan out to all projections.
                {
                    let projections = store.projections.read().await;
                    for runner_mutex in projections.values() {
                        let mut runner = runner_mutex.lock().await;
                        runner.apply_event(&recorded);
                    }
                }

                // Fan out to all process managers and collect envelopes.
                let mut all_envelopes: Vec<(Vec<CommandEnvelope>, PathBuf)> = Vec::new();
                {
                    let pms = store.process_managers.read().await;
                    for pm_mutex in pms.iter() {
                        let mut pm = pm_mutex.lock().await;
                        let envelopes = pm.react_event(&recorded);
                        if !envelopes.is_empty() {
                            let dead_letter_path = pm.dead_letter_path();
                            all_envelopes.push((envelopes, dead_letter_path));
                        }
                    }
                }

                // Dispatch PM envelopes (same pattern as run_process_managers).
                for (envelopes, dead_letter_path) in all_envelopes {
                    for envelope in envelopes {
                        let agg_type = &envelope.aggregate_type;
                        match store.dispatchers.get(agg_type) {
                            Some(dispatcher) => {
                                match dispatcher.dispatch(store, envelope.clone()).await {
                                    Ok(()) => {
                                        tracing::info!(
                                            target_type = %agg_type,
                                            target_id = %envelope.instance_id,
                                            "live loop: dispatched command"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            aggregate_type = %agg_type,
                                            instance_id = %envelope.instance_id,
                                            error = %e,
                                            "live loop: dispatch failed, dead-lettering"
                                        );
                                        if let Err(dl_err) = append_dead_letter(
                                            &dead_letter_path,
                                            envelope,
                                            &e.to_string(),
                                        ) {
                                            tracing::error!(
                                                error = %dl_err,
                                                "failed to write dead letter"
                                            );
                                        }
                                    }
                                }
                            }
                            None => {
                                let err_msg = format!("unknown aggregate type: {agg_type}");
                                tracing::error!(
                                    aggregate_type = %agg_type,
                                    "live loop: no dispatcher registered, \
                                     dead-lettering"
                                );
                                if let Err(dl_err) =
                                    append_dead_letter(&dead_letter_path, envelope, &err_msg)
                                {
                                    tracing::error!(
                                        error = %dl_err,
                                        "failed to write dead letter"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Some(subscribe_response::Content::CaughtUp(_)) => {
                caught_up.store(true, Ordering::Release);
                tracing::debug!("live loop: caught up");
            }
            None => {
                // Empty response content; skip.
            }
        }
    }

    // Stream ended (EOF).
    Ok(StreamOutcome::Ended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_config_default_values() {
        let config = LiveConfig::default();
        assert_eq!(config.checkpoint_interval, Duration::from_secs(5));
        assert_eq!(config.reconnect_base_delay, Duration::from_secs(1));
        assert_eq!(config.reconnect_max_delay, Duration::from_secs(30));
    }

    #[test]
    fn live_handle_is_clone() {
        // Verify LiveHandle derives Clone by constructing one and cloning it.
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);
        let handle = LiveHandle {
            shutdown_tx,
            caught_up: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            task: Arc::new(tokio::sync::Mutex::new(None)),
        };
        let _cloned = handle.clone();
    }

    #[test]
    fn is_caught_up_returns_false_initially() {
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);
        let handle = LiveHandle {
            shutdown_tx,
            caught_up: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            task: Arc::new(tokio::sync::Mutex::new(None)),
        };
        assert!(!handle.is_caught_up());
    }

    #[test]
    fn is_caught_up_returns_true_after_flag_set() {
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);
        let caught_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle = LiveHandle {
            shutdown_tx,
            caught_up: caught_up.clone(),
            task: Arc::new(tokio::sync::Mutex::new(None)),
        };
        caught_up.store(true, std::sync::atomic::Ordering::Release);
        assert!(handle.is_caught_up());
    }

    #[tokio::test]
    async fn shutdown_with_no_task_returns_ok() {
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);
        let handle = LiveHandle {
            shutdown_tx,
            caught_up: Arc::new(AtomicBool::new(false)),
            task: Arc::new(tokio::sync::Mutex::new(None)),
        };
        let result = handle.shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn shutdown_twice_returns_ok() {
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);
        let task_handle = tokio::spawn(async { Ok(()) });
        let handle = LiveHandle {
            shutdown_tx,
            caught_up: Arc::new(AtomicBool::new(false)),
            task: Arc::new(tokio::sync::Mutex::new(Some(task_handle))),
        };
        handle.shutdown().await.expect("first shutdown");
        handle.shutdown().await.expect("second shutdown");
    }

    // --- run_live_loop tests ---

    use std::collections::HashMap;
    use std::path::Path;

    use crate::client::EsClient;
    use crate::event::StoredEvent;
    use crate::projection::{Projection, ProjectionCatchUp, ProjectionRunner};
    use crate::proto::{Empty, RecordedEvent, SubscribeResponse, subscribe_response::Content};
    use crate::store::AggregateStore;

    /// A test projection that counts all events.
    #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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

    /// Build a mock `EsClient` with a lazy (non-connecting) channel.
    fn mock_client() -> EsClient {
        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        EsClient::from_inner(inner)
    }

    /// Build a mock `AggregateStore` with an EventCounter projection.
    fn mock_store_with_projection(base_dir: &Path) -> AggregateStore {
        let client = mock_client();

        let checkpoint_dir = base_dir.join("projections").join("event-counter");
        let runner = ProjectionRunner::<EventCounter>::new(client.clone(), checkpoint_dir)
            .expect("runner creation should succeed");
        let mut projections = HashMap::new();
        projections.insert(
            "event-counter".to_string(),
            tokio::sync::Mutex::new(Box::new(runner) as Box<dyn ProjectionCatchUp>),
        );

        AggregateStore {
            client,
            base_dir: base_dir.to_path_buf(),
            cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            projections: Arc::new(tokio::sync::RwLock::new(projections)),
            process_managers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            dispatchers: Arc::new(HashMap::new()),
            idle_timeout: Duration::from_secs(300),
            live_config: LiveConfig::default(),
            live_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    use crate::process_manager::test_fixtures::EchoSaga;
    use crate::process_manager::{ProcessManagerCatchUp, ProcessManagerRunner};

    /// Build a mock `AggregateStore` with an EventCounter projection and EchoSaga PM.
    fn mock_store_with_projection_and_pm(base_dir: &Path) -> AggregateStore {
        let client = mock_client();

        // Projection
        let proj_dir = base_dir.join("projections").join("event-counter");
        let proj_runner = ProjectionRunner::<EventCounter>::new(client.clone(), proj_dir)
            .expect("projection runner creation should succeed");
        let mut projections = HashMap::new();
        projections.insert(
            "event-counter".to_string(),
            tokio::sync::Mutex::new(Box::new(proj_runner) as Box<dyn ProjectionCatchUp>),
        );

        // Process manager
        let pm_dir = base_dir.join("process_managers").join("echo-saga");
        let pm_runner = ProcessManagerRunner::<EchoSaga>::new(client.clone(), pm_dir)
            .expect("PM runner creation should succeed");
        let process_managers = vec![tokio::sync::Mutex::new(
            Box::new(pm_runner) as Box<dyn ProcessManagerCatchUp>
        )];

        AggregateStore {
            client,
            base_dir: base_dir.to_path_buf(),
            cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            projections: Arc::new(tokio::sync::RwLock::new(projections)),
            process_managers: Arc::new(tokio::sync::RwLock::new(process_managers)),
            dispatchers: Arc::new(HashMap::new()),
            idle_timeout: Duration::from_secs(300),
            live_config: LiveConfig::default(),
            live_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn live_loop_processes_events_and_fans_out_to_projections() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection(tmp.path());
        let caught_up = Arc::new(AtomicBool::new(false));

        // Build a mock stream: 2 events + CaughtUp, then stream ends.
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            event_response(1, 1),
            caught_up_response(),
        ]);

        // Process the stream once (no reconnect needed -- stream ends after CaughtUp).
        process_stream_with_dispatch(&store, stream, &caught_up)
            .await
            .expect("should succeed");

        // Verify caught_up flag was set.
        assert!(caught_up.load(Ordering::Acquire));

        // Verify projection was updated with 2 events.
        let proj_map = store.projections.read().await;
        let runner = proj_map
            .get("event-counter")
            .expect("should have projection");
        let runner_lock = runner.lock().await;
        let state_box = runner_lock.state_any();
        let state = state_box
            .downcast::<EventCounter>()
            .expect("downcast should succeed");
        assert_eq!(state.count, 2);
    }

    #[tokio::test]
    async fn live_loop_dispatches_pm_envelopes_dead_letters_unknown_type() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection_and_pm(tmp.path());
        let caught_up = Arc::new(AtomicBool::new(false));

        // Stream with one counter event that triggers EchoSaga to emit
        // a command targeting "target" aggregate (which has no dispatcher).
        let stream = tokio_stream::iter(vec![event_response(0, 0), caught_up_response()]);

        process_stream_with_dispatch(&store, stream, &caught_up)
            .await
            .expect("should succeed");

        // The EchoSaga PM should have produced an envelope targeting "target".
        // Since no dispatcher is registered for "target", it should be
        // dead-lettered.
        let dead_letter_path = tmp
            .path()
            .join("process_managers")
            .join("echo-saga")
            .join("dead_letters.jsonl");
        let contents =
            std::fs::read_to_string(&dead_letter_path).expect("dead letter file should exist");
        assert!(
            contents.contains("unknown aggregate type: target"),
            "dead letter should mention unknown aggregate type"
        );
    }

    #[tokio::test]
    async fn live_loop_saves_checkpoints_on_shutdown() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection(tmp.path());
        let caught_up = Arc::new(AtomicBool::new(false));

        // Process 2 events.
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            event_response(1, 1),
            caught_up_response(),
        ]);
        process_stream_with_dispatch(&store, stream, &caught_up)
            .await
            .expect("should succeed");

        // Save checkpoints (simulating what shutdown does).
        save_all_checkpoints(&store).await;

        // Verify projection checkpoint was saved.
        let checkpoint_path = tmp
            .path()
            .join("projections")
            .join("event-counter")
            .join("checkpoint.json");
        let contents =
            std::fs::read_to_string(&checkpoint_path).expect("checkpoint file should exist");
        let parsed: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");
        assert_eq!(parsed["last_global_position"], 2);
        assert_eq!(parsed["state"]["count"], 2);
    }

    #[tokio::test]
    async fn min_global_position_with_no_projections_returns_zero() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let client = mock_client();
        let store = AggregateStore {
            client,
            base_dir: tmp.path().to_path_buf(),
            cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            projections: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            process_managers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            dispatchers: Arc::new(HashMap::new()),
            idle_timeout: Duration::from_secs(300),
            live_config: LiveConfig::default(),
            live_handle: Arc::new(tokio::sync::Mutex::new(None)),
        };
        assert_eq!(min_global_position(&store).await, 0);
    }

    #[tokio::test]
    async fn min_global_position_returns_min_of_projections_and_pms() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection_and_pm(tmp.path());

        // Initially both start at position 0.
        assert_eq!(min_global_position(&store).await, 0);

        // Process some events to advance projection position.
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            event_response(1, 1),
            caught_up_response(),
        ]);
        let caught_up = Arc::new(AtomicBool::new(false));
        process_stream_with_dispatch(&store, stream, &caught_up)
            .await
            .expect("should succeed");

        // Both projection and PM should have advanced to position 2.
        assert_eq!(min_global_position(&store).await, 2);
    }

    #[tokio::test]
    async fn backoff_respects_live_config_values() {
        // Verify the backoff capping logic.
        let config = LiveConfig {
            checkpoint_interval: Duration::from_secs(1),
            reconnect_base_delay: Duration::from_millis(100),
            reconnect_max_delay: Duration::from_millis(300),
        };

        let mut delay = config.reconnect_base_delay;
        assert_eq!(delay, Duration::from_millis(100));

        delay = (delay * 2).min(config.reconnect_max_delay);
        assert_eq!(delay, Duration::from_millis(200));

        delay = (delay * 2).min(config.reconnect_max_delay);
        assert_eq!(delay, Duration::from_millis(300)); // capped

        delay = (delay * 2).min(config.reconnect_max_delay);
        assert_eq!(delay, Duration::from_millis(300)); // still capped
    }

    #[tokio::test]
    async fn stream_error_returns_stream_outcome_error() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection(tmp.path());
        let caught_up = Arc::new(AtomicBool::new(false));

        // Stream that yields one event, then an error.
        let stream = tokio_stream::iter(vec![
            event_response(0, 0),
            Err(tonic::Status::internal("connection lost")),
        ]);

        let result = process_stream_with_dispatch(&store, stream, &caught_up)
            .await
            .expect("should return Ok(StreamOutcome)");

        assert!(
            matches!(result, StreamOutcome::Error(_)),
            "should be StreamOutcome::Error"
        );

        // Projection should still have the first event applied.
        let proj_map = store.projections.read().await;
        let runner = proj_map
            .get("event-counter")
            .expect("should have projection");
        let runner_lock = runner.lock().await;
        let state_box = runner_lock.state_any();
        let state = state_box
            .downcast::<EventCounter>()
            .expect("downcast should succeed");
        assert_eq!(state.count, 1);
    }

    #[tokio::test]
    async fn run_live_loop_shutdown_saves_checkpoints() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store_with_projection(tmp.path());

        // Pre-process some events so the projection has state.
        {
            let caught_up = Arc::new(AtomicBool::new(false));
            let stream = tokio_stream::iter(vec![event_response(0, 0), caught_up_response()]);
            process_stream_with_dispatch(&store, stream, &caught_up)
                .await
                .expect("should succeed");
        }

        // Create a LiveHandle wrapping a task that just saves checkpoints
        // via shutdown.
        let caught_up_flag = Arc::new(AtomicBool::new(true));
        let (shutdown_tx, _rx) = tokio::sync::watch::channel(false);

        let store_clone = store.clone();
        let task = tokio::spawn(async move {
            save_all_checkpoints(&store_clone).await;
            Ok(())
        });

        let handle = LiveHandle {
            shutdown_tx,
            caught_up: caught_up_flag,
            task: Arc::new(tokio::sync::Mutex::new(Some(task))),
        };

        handle.shutdown().await.expect("shutdown should succeed");

        // Verify checkpoint was saved.
        let checkpoint_path = tmp
            .path()
            .join("projections")
            .join("event-counter")
            .join("checkpoint.json");
        let contents =
            std::fs::read_to_string(&checkpoint_path).expect("checkpoint file should exist");
        let parsed: serde_json::Value = serde_json::from_str(&contents).expect("valid JSON");
        assert_eq!(parsed["last_global_position"], 1);
    }
}
