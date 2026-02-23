//! Top-level entry point that composes storage layout, actor spawning, and
//! handle caching into a single `AggregateStore` type.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::actor::{ActorConfig, AggregateHandle, spawn_actor_with_config};
use crate::aggregate::Aggregate;
use crate::process_manager::{
    AggregateDispatcher, ProcessManagerCatchUp, ProcessManagerReport, ProcessManagerRunner,
    TypedDispatcher, append_dead_letter,
};
use crate::projection::{Projection, ProjectionRunner};
use crate::storage::StreamLayout;

/// Type-erased handle cache keyed by `(aggregate_type, instance_id)`.
///
/// `Box<dyn Any + Send + Sync>` lets a single map hold `AggregateHandle<A>`
/// for any concrete `A`. Downcasting recovers the typed handle.
type HandleCache = HashMap<(String, String), Box<dyn Any + Send + Sync>>;

/// Type-erased projection map keyed by projection name.
///
/// Each value is a `std::sync::Mutex<ProjectionRunner<P>>` erased to `dyn Any`.
/// We use `std::sync::Mutex` (not `tokio::sync::Mutex`) because the lock is
/// held briefly and `catch_up` does blocking I/O that should not hold an async
/// mutex across an `.await` point.
type ProjectionMap = HashMap<String, Box<dyn Any + Send + Sync>>;

/// Type-erased list of process manager runners.
///
/// Each runner is wrapped in `std::sync::Mutex` because `catch_up` does
/// blocking file I/O that must not hold an async mutex.
type ProcessManagerList = Vec<std::sync::Mutex<Box<dyn ProcessManagerCatchUp>>>;

/// Type-erased dispatcher map keyed by aggregate type name.
type DispatcherMap = HashMap<String, Box<dyn AggregateDispatcher>>;

/// Type-erased catch-up list for projections.
///
/// Mirrors the `ProcessManagerList` pattern: each entry wraps a
/// `ProjectionRunner<P>` behind a `std::sync::Mutex` so that
/// [`inject_event`](AggregateStore::inject_event) can trigger catch-up on
/// all projections without knowing the concrete `P` type.
type ProjectionCatchUpList = Vec<std::sync::Mutex<Box<dyn ProjectionCatchUpFn>>>;

/// Type-erased projection catch-up interface.
///
/// Implemented by [`ProjectionRunner<P>`](crate::projection::ProjectionRunner)
/// wrapper so that the store can catch up all projections without knowing
/// the concrete `P` type parameter.
trait ProjectionCatchUpFn: Send + Sync {
    /// Catch up on all subscribed streams and save the checkpoint.
    fn catch_up(&mut self) -> io::Result<()>;
}

/// Wrapper that implements [`ProjectionCatchUpFn`] by delegating to a
/// shared `Arc<Mutex<ProjectionRunner<P>>>`. This allows the type-erased
/// catch-up list and the typed projection map to share the same runner.
struct SharedProjectionCatchUp<P: Projection> {
    inner: Arc<std::sync::Mutex<ProjectionRunner<P>>>,
}

impl<P: Projection> ProjectionCatchUpFn for SharedProjectionCatchUp<P> {
    fn catch_up(&mut self) -> io::Result<()> {
        let mut runner = self
            .inner
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        runner.catch_up()
    }
}

/// Default idle timeout for actors: 5 minutes.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Options controlling the behaviour of [`AggregateStore::inject_event`].
///
/// # Examples
///
/// ```
/// use eventfold_es::InjectOptions;
///
/// // Default: do not run process managers after injection.
/// let opts = InjectOptions::default();
/// assert!(!opts.run_process_managers);
///
/// // Opt in to process manager triggering.
/// let opts = InjectOptions { run_process_managers: true };
/// assert!(opts.run_process_managers);
/// ```
#[derive(Debug, Clone, Default)]
pub struct InjectOptions {
    /// When `true`, call [`AggregateStore::run_process_managers`] after
    /// appending the event. Defaults to `false`.
    pub run_process_managers: bool,
}

/// Central registry that manages aggregate instance lifecycles.
///
/// The store handles directory creation, actor spawning, and handle caching.
/// It is `Clone + Send + Sync` -- cloning shares the underlying cache.
///
/// # Examples
///
/// ```no_run
/// use eventfold_es::{AggregateStore, CommandContext};
///
/// # async fn example() -> std::io::Result<()> {
/// let store = AggregateStore::open("/tmp/my-app").await?;
/// // Use store.get::<MyAggregate>("instance-id") to get handles
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct AggregateStore {
    layout: StreamLayout,
    cache: Arc<RwLock<HandleCache>>,
    projections: Arc<std::sync::RwLock<ProjectionMap>>,
    /// Type-erased projection catch-up runners for [`inject_event`].
    projection_catch_ups: Arc<std::sync::RwLock<ProjectionCatchUpList>>,
    process_managers: Arc<std::sync::RwLock<ProcessManagerList>>,
    dispatchers: Arc<DispatcherMap>,
    /// In-memory set of event IDs already injected, for deduplication.
    /// Shared across clones via `Arc`.
    seen_ids: Arc<std::sync::Mutex<HashSet<String>>>,
    idle_timeout: Duration,
}

// Manual `Debug` because `dyn Any` is not `Debug` and we don't want to
// expose cache internals.
impl std::fmt::Debug for AggregateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AggregateStore")
            .field("base_dir", &self.layout.base_dir())
            .finish()
    }
}

impl AggregateStore {
    /// Open or create a store rooted at `base_dir`.
    ///
    /// Creates the metadata directory if it does not exist.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Root directory for all event store data.
    ///
    /// # Returns
    ///
    /// A new `AggregateStore` ready to spawn aggregate actors.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the metadata directory cannot be created.
    pub async fn open(base_dir: impl AsRef<Path>) -> io::Result<Self> {
        let layout = StreamLayout::new(base_dir.as_ref());
        // Create meta dir using blocking I/O wrapped in spawn_blocking
        // to avoid blocking the tokio reactor.
        let meta_dir = layout.meta_dir();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(meta_dir))
            .await
            .map_err(io::Error::other)??;
        Ok(Self {
            layout,
            cache: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(std::sync::RwLock::new(HashMap::new())),
            projection_catch_ups: Arc::new(std::sync::RwLock::new(Vec::new())),
            process_managers: Arc::new(std::sync::RwLock::new(Vec::new())),
            dispatchers: Arc::new(HashMap::new()),
            seen_ids: Arc::new(std::sync::Mutex::new(HashSet::new())),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        })
    }

    /// Get a handle to an aggregate instance, spawning its actor if needed.
    ///
    /// If the actor is already running (cached), returns a clone of the
    /// existing handle. Otherwise, creates the stream directory on disk and
    /// spawns a new actor thread.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique instance identifier within the aggregate type.
    ///
    /// # Returns
    ///
    /// An [`AggregateHandle`] for sending commands and reading state.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directory creation or event log opening fails.
    pub async fn get<A: Aggregate>(&self, id: &str) -> io::Result<AggregateHandle<A>> {
        let key = (A::AGGREGATE_TYPE.to_owned(), id.to_owned());

        // Fast path: check cache with read lock.
        {
            let cache = self.cache.read().await;
            if let Some(boxed) = cache.get(&key)
                && let Some(handle) = boxed.downcast_ref::<AggregateHandle<A>>()
                && handle.is_alive()
            {
                return Ok(handle.clone());
            }
        }

        // If we get here, either the handle is missing or the actor has
        // exited (e.g. idle timeout). Evict any stale entry and re-spawn.
        {
            let mut cache = self.cache.write().await;
            cache.remove(&key);
        }

        // Slow path: create stream directory and spawn actor.
        let layout = self.layout.clone();
        let agg_type = A::AGGREGATE_TYPE.to_owned();
        let inst_id = id.to_owned();
        let stream_dir =
            tokio::task::spawn_blocking(move || layout.ensure_stream(&agg_type, &inst_id))
                .await
                .map_err(io::Error::other)??;

        tracing::debug!(
            aggregate_type = A::AGGREGATE_TYPE,
            instance_id = %id,
            "spawning actor"
        );

        let config = ActorConfig {
            idle_timeout: self.idle_timeout,
        };
        let handle = spawn_actor_with_config::<A>(&stream_dir, config)?;

        let mut cache = self.cache.write().await;
        cache.insert(key, Box::new(handle.clone()));
        Ok(handle)
    }

    /// List all instance IDs for a given aggregate type.
    ///
    /// # Returns
    ///
    /// A sorted `Vec<String>` of instance IDs. Returns an empty vector
    /// if no instances of the given aggregate type exist.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if reading the directory fails.
    pub async fn list<A: Aggregate>(&self) -> io::Result<Vec<String>> {
        let layout = self.layout.clone();
        let agg_type = A::AGGREGATE_TYPE.to_owned();
        tokio::task::spawn_blocking(move || layout.list_streams(&agg_type))
            .await
            .map_err(io::Error::other)?
    }

    /// Create a builder for configuring projections and other options.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Root directory for all event store data.
    ///
    /// # Returns
    ///
    /// An [`AggregateStoreBuilder`] that can register projections before opening.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use eventfold_es::AggregateStore;
    ///
    /// # async fn example() -> std::io::Result<()> {
    /// let store = AggregateStore::builder("/tmp/my-app")
    ///     // .projection::<MyProjection>()
    ///     .open()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder(base_dir: impl AsRef<Path>) -> AggregateStoreBuilder {
        AggregateStoreBuilder {
            base_dir: base_dir.as_ref().to_owned(),
            projection_factories: Vec::new(),
            process_manager_factories: Vec::new(),
            dispatcher_factories: Vec::new(),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Catch up and return the current state of a registered projection.
    ///
    /// Triggers a lazy catch-up before returning: reads any new events
    /// from subscribed streams. Returns a clone of the projection state.
    ///
    /// This method is synchronous (not async). It uses `std::sync` locks
    /// and blocking I/O internally. For embedded use, `catch_up` is fast
    /// for incremental updates. Callers that need an async boundary can
    /// wrap this in `tokio::task::spawn_blocking`.
    ///
    /// # Returns
    ///
    /// A clone of the projection's current state after catching up.
    ///
    /// # Errors
    ///
    /// Returns `io::ErrorKind::NotFound` if the projection is not registered.
    /// Returns `io::Error` if catching up on events fails.
    pub fn projection<P: Projection>(&self) -> io::Result<P> {
        let projections = self
            .projections
            .read()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let runner_any = projections.get(P::NAME).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("projection '{}' not registered", P::NAME),
            )
        })?;
        // Downcast the type-erased `Box<dyn Any>` back to the concrete
        // `Arc<Mutex<ProjectionRunner<P>>>`. This is safe because
        // `projection()` on the builder registered the runner under the
        // same `P::NAME` key.
        let runner_arc = runner_any
            .downcast_ref::<Arc<std::sync::Mutex<ProjectionRunner<P>>>>()
            .ok_or_else(|| io::Error::other("projection type mismatch"))?;
        let mut runner = runner_arc
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        runner.catch_up()?;
        Ok(runner.state().clone())
    }

    /// Delete the checkpoint for projection `P` and replay all events from scratch.
    ///
    /// This acquires the projections read-lock, downcasts to the concrete
    /// `ProjectionRunner<P>`, and calls `rebuild()` which:
    /// 1. Deletes the checkpoint file
    /// 2. Resets internal state to `ProjectionCheckpoint::default()`
    /// 3. Calls `catch_up()` to replay all events from offset 0
    /// 4. Saves the new checkpoint
    ///
    /// **Blocking I/O** -- if called from an async context,
    /// wrap this in `tokio::task::spawn_blocking`.
    ///
    /// # Errors
    ///
    /// Returns `io::ErrorKind::NotFound` if the projection is not registered.
    /// Returns `io::Error` if deleting the checkpoint or catching up fails.
    pub fn rebuild_projection<P: Projection>(&self) -> io::Result<()> {
        let projections = self
            .projections
            .read()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let runner_any = projections.get(P::NAME).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("projection '{}' not registered", P::NAME),
            )
        })?;
        let runner_arc = runner_any
            .downcast_ref::<Arc<std::sync::Mutex<ProjectionRunner<P>>>>()
            .ok_or_else(|| io::Error::other("projection type mismatch"))?;
        let mut runner = runner_arc
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        runner.rebuild()
    }

    /// Run all registered process managers through a catch-up pass.
    ///
    /// For each process manager:
    /// 1. Catch up on subscribed streams, collecting command envelopes.
    /// 2. Dispatch each envelope to the target aggregate via the type registry.
    /// 3. Write failed dispatches to the per-PM dead-letter log.
    /// 4. Save the process manager checkpoint after all envelopes are handled.
    ///
    /// # Returns
    ///
    /// A [`ProcessManagerReport`] summarizing how many envelopes were
    /// dispatched and how many were dead-lettered.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if catching up or saving checkpoints fails.
    pub async fn run_process_managers(&self) -> io::Result<ProcessManagerReport> {
        // Collect envelopes and dead-letter paths from each PM under the
        // std::sync::Mutex. The lock is held only for catch_up (blocking I/O).
        let mut all_work: Vec<(Vec<crate::command::CommandEnvelope>, std::path::PathBuf)> =
            Vec::new();

        {
            let pms = self
                .process_managers
                .read()
                .map_err(|e| io::Error::other(e.to_string()))?;
            for pm_mutex in pms.iter() {
                let mut pm = pm_mutex
                    .lock()
                    .map_err(|e| io::Error::other(e.to_string()))?;
                let envelopes = pm.catch_up()?;
                let dead_letter_path = pm.dead_letter_path();
                all_work.push((envelopes, dead_letter_path));
            }
        }

        // Dispatch envelopes asynchronously.
        let mut report = ProcessManagerReport::default();
        for (envelopes, dead_letter_path) in &all_work {
            for envelope in envelopes {
                let agg_type = &envelope.aggregate_type;
                match self.dispatchers.get(agg_type) {
                    Some(dispatcher) => match dispatcher.dispatch(self, envelope.clone()).await {
                        Ok(()) => {
                            tracing::info!(
                                target_type = %agg_type,
                                target_id = %envelope.instance_id,
                                "dispatching command"
                            );
                            report.dispatched += 1;
                        }
                        Err(e) => {
                            tracing::error!(
                                aggregate_type = %agg_type,
                                instance_id = %envelope.instance_id,
                                error = %e,
                                "process manager dispatch failed, dead-lettering"
                            );
                            append_dead_letter(dead_letter_path, envelope.clone(), &e.to_string())?;
                            report.dead_lettered += 1;
                        }
                    },
                    None => {
                        let err_msg = format!("unknown aggregate type: {agg_type}");
                        tracing::error!(
                            aggregate_type = %agg_type,
                            "no dispatcher registered, dead-lettering"
                        );
                        append_dead_letter(dead_letter_path, envelope.clone(), &err_msg)?;
                        report.dead_lettered += 1;
                    }
                }
            }
        }

        // Save all PM checkpoints after dispatch is complete.
        {
            let pms = self
                .process_managers
                .read()
                .map_err(|e| io::Error::other(e.to_string()))?;
            for pm_mutex in pms.iter() {
                let pm = pm_mutex
                    .lock()
                    .map_err(|e| io::Error::other(e.to_string()))?;
                pm.save()?;
            }
        }

        Ok(report)
    }

    /// Returns a reference to the underlying storage layout.
    pub fn layout(&self) -> &StreamLayout {
        &self.layout
    }

    /// List all known `(aggregate_type, instance_id)` pairs.
    ///
    /// When `aggregate_type` is `Some`, returns only streams for that type.
    /// When `None`, returns streams across all aggregate types. Results are
    /// sorted by aggregate type then instance ID.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - Optional filter. When `Some`, only streams for
    ///   that aggregate type are returned. When `None`, all streams are
    ///   returned.
    ///
    /// # Returns
    ///
    /// A sorted `Vec<(String, String)>` of `(aggregate_type, instance_id)`
    /// pairs. Returns an empty vector if no matching streams exist.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if reading the directory fails.
    pub async fn list_streams(
        &self,
        aggregate_type: Option<&str>,
    ) -> io::Result<Vec<(String, String)>> {
        let layout = self.layout.clone();
        match aggregate_type {
            Some(agg_type) => {
                let agg_type = agg_type.to_owned();
                tokio::task::spawn_blocking(move || {
                    let ids = layout.list_streams(&agg_type)?;
                    Ok(ids.into_iter().map(|id| (agg_type.clone(), id)).collect())
                })
                .await
                .map_err(io::Error::other)?
            }
            None => tokio::task::spawn_blocking(move || {
                let types = layout.list_aggregate_types()?;
                let mut pairs = Vec::new();
                for agg_type in types {
                    let ids = layout.list_streams(&agg_type)?;
                    pairs.extend(ids.into_iter().map(|id| (agg_type.clone(), id)));
                }
                Ok(pairs)
            })
            .await
            .map_err(io::Error::other)?,
        }
    }

    /// Read all raw events from a stream identified by aggregate type and
    /// instance ID.
    ///
    /// Returns the events in the order they were appended. Does not spawn
    /// an actor or acquire a write lock on the stream.
    ///
    /// # Arguments
    ///
    /// * `aggregate_type` - The aggregate type name (e.g. `"counter"`).
    /// * `instance_id` - The unique instance identifier within that type.
    ///
    /// # Returns
    ///
    /// A `Vec<eventfold::Event>` containing all events in the stream.
    /// Returns `Ok(vec![])` if the stream directory exists but no events
    /// have been written yet.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` with `ErrorKind::NotFound` if the stream
    /// directory does not exist (i.e. the stream was never created).
    /// Returns `std::io::Error` for other I/O failures during reading.
    pub async fn read_events(
        &self,
        aggregate_type: &str,
        instance_id: &str,
    ) -> io::Result<Vec<eventfold::Event>> {
        let layout = self.layout.clone();
        let agg_type = aggregate_type.to_owned();
        let inst_id = instance_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let stream_dir = layout.stream_dir(&agg_type, &inst_id);

            // If the stream directory itself does not exist, return NotFound.
            if !stream_dir.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("stream directory not found: {}", stream_dir.display()),
                ));
            }

            let reader = eventfold::EventReader::new(&stream_dir);
            // read_from(0) returns NotFound when app.jsonl doesn't exist yet
            // (stream dir created but no events written). Map that to empty vec.
            let iter = match reader.read_from(0) {
                Ok(iter) => iter,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(e),
            };

            let mut events = Vec::new();
            for result in iter {
                let (event, _next_offset, _line_hash) = result?;
                events.push(event);
            }
            Ok(events)
        })
        .await
        .map_err(io::Error::other)?
    }

    /// Append a pre-validated event directly to a stream, bypassing command
    /// validation.
    ///
    /// This is the primary entry point for relay-sync scenarios where events
    /// have already been validated on the originating client. The event is
    /// written as-is to the stream's JSONL log, projections are caught up,
    /// and process managers are optionally triggered.
    ///
    /// # Deduplication
    ///
    /// If `event.id` is `Some(id)` and that ID has already been seen by this
    /// store instance, the call returns `Ok(())` immediately without writing.
    /// Events with `event.id = None` are never deduplicated.
    ///
    /// # Actor interaction
    ///
    /// If a live actor exists for the target stream, the event is injected
    /// through the actor's channel (preserving the actor's exclusive writer
    /// ownership). Otherwise, a temporary `EventWriter` is opened directly.
    ///
    /// # Arguments
    ///
    /// * `instance_id` - Unique instance identifier within the aggregate type.
    /// * `event` - A pre-validated `eventfold::Event` to append as-is.
    /// * `opts` - Controls whether process managers run after injection.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success (including dedup no-ops).
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if directory creation, event writing, or
    /// projection catch-up fails.
    pub async fn inject_event<A: Aggregate>(
        &self,
        instance_id: &str,
        event: eventfold::Event,
        opts: InjectOptions,
    ) -> io::Result<()> {
        // 1. Dedup check: if the event has an ID already seen, no-op.
        let event_id = event.id.clone();
        if let Some(ref id) = event_id {
            let seen = self
                .seen_ids
                .lock()
                .map_err(|e| io::Error::other(e.to_string()))?;
            if seen.contains(id) {
                return Ok(());
            }
        }

        // 2. Ensure stream directory exists.
        let layout = self.layout.clone();
        let agg_type = A::AGGREGATE_TYPE.to_owned();
        let inst_id = instance_id.to_owned();
        let stream_dir =
            tokio::task::spawn_blocking(move || layout.ensure_stream(&agg_type, &inst_id))
                .await
                .map_err(io::Error::other)??;

        // 3. Append the event: route through the actor if one is alive,
        //    otherwise open a temporary writer directly.
        let key = (A::AGGREGATE_TYPE.to_owned(), instance_id.to_owned());
        let injected_via_actor = {
            let cache = self.cache.read().await;
            if let Some(boxed) = cache.get(&key)
                && let Some(handle) = boxed.downcast_ref::<AggregateHandle<A>>()
                && handle.is_alive()
            {
                handle.inject_via_actor(event.clone()).await?;
                true
            } else {
                false
            }
        };

        if !injected_via_actor {
            let ev = event;
            tokio::task::spawn_blocking(move || {
                let mut writer = eventfold::EventWriter::open(&stream_dir)?;
                writer.append(&ev).map(|_| ())
            })
            .await
            .map_err(io::Error::other)??;
        }

        // 4. Register event ID in seen_ids after successful append.
        if let Some(id) = event_id {
            let mut seen = self
                .seen_ids
                .lock()
                .map_err(|e| io::Error::other(e.to_string()))?;
            seen.insert(id);
        }

        // 5. Catch up all registered projections via the type-erased list.
        {
            let catch_ups = self
                .projection_catch_ups
                .read()
                .map_err(|e| io::Error::other(e.to_string()))?;
            for catch_up_mutex in catch_ups.iter() {
                let mut catch_up = catch_up_mutex
                    .lock()
                    .map_err(|e| io::Error::other(e.to_string()))?;
                catch_up.catch_up()?;
            }
        }

        // 6. Optionally trigger process managers.
        if opts.run_process_managers {
            self.run_process_managers().await?;
        }

        Ok(())
    }
}

/// Factory function type for creating a type-erased projection runner.
///
/// Each closure captures the concrete `P: Projection` type, creates a
/// `ProjectionRunner<P>`, and returns both:
/// - A `Box<dyn Any + Send + Sync>` (the `Arc<Mutex<ProjectionRunner<P>>>`) for
///   the typed projection map (used by `projection::<P>()`).
/// - A `Mutex<Box<dyn ProjectionCatchUpFn>>` for the type-erased catch-up list
///   (used by `inject_event`).
type ProjectionFactory = Box<
    dyn FnOnce(
        StreamLayout,
    ) -> io::Result<(
        Box<dyn Any + Send + Sync>,
        std::sync::Mutex<Box<dyn ProjectionCatchUpFn>>,
    )>,
>;

/// Factory function type for creating a type-erased process manager runner.
type ProcessManagerFactory =
    Box<dyn FnOnce(StreamLayout) -> io::Result<std::sync::Mutex<Box<dyn ProcessManagerCatchUp>>>>;

/// Factory function type for creating a type-erased aggregate dispatcher.
type DispatcherFactory = Box<dyn FnOnce() -> Box<dyn AggregateDispatcher>>;

/// Builder for configuring an [`AggregateStore`] with projections and
/// process managers.
///
/// Created via [`AggregateStore::builder`]. Register projections with
/// [`projection`](AggregateStoreBuilder::projection), process managers with
/// [`process_manager`](AggregateStoreBuilder::process_manager), and dispatch
/// targets with [`aggregate_type`](AggregateStoreBuilder::aggregate_type),
/// then call [`open`](AggregateStoreBuilder::open) to finalize.
///
/// # Examples
///
/// ```no_run
/// use eventfold_es::AggregateStore;
///
/// # async fn example() -> std::io::Result<()> {
/// let store = AggregateStore::builder("/tmp/my-app")
///     // .projection::<MyProjection>()
///     // .process_manager::<MySaga>()
///     // .aggregate_type::<MyAggregate>()
///     .open()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct AggregateStoreBuilder {
    base_dir: PathBuf,
    projection_factories: Vec<(String, ProjectionFactory)>,
    process_manager_factories: Vec<(String, ProcessManagerFactory)>,
    dispatcher_factories: Vec<(String, DispatcherFactory)>,
    idle_timeout: Duration,
}

impl AggregateStoreBuilder {
    /// Register a projection type to be managed by this store.
    ///
    /// The projection will be initialized (loading any existing checkpoint)
    /// when [`open`](AggregateStoreBuilder::open) is called.
    ///
    /// # Type Parameters
    ///
    /// * `P` - A type implementing [`Projection`].
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn projection<P: Projection>(mut self) -> Self {
        self.projection_factories.push((
            P::NAME.to_owned(),
            Box::new(|layout| {
                let runner = ProjectionRunner::<P>::new(layout)?;
                let shared = Arc::new(std::sync::Mutex::new(runner));
                // Store the Arc in the typed projection map for downcasting
                // by `AggregateStore::projection::<P>()`.
                let any_box: Box<dyn Any + Send + Sync> = Box::new(shared.clone());
                // Create a type-erased catch-up wrapper sharing the same runner.
                let catch_up: std::sync::Mutex<Box<dyn ProjectionCatchUpFn>> =
                    std::sync::Mutex::new(Box::new(SharedProjectionCatchUp { inner: shared }));
                Ok((any_box, catch_up))
            }),
        ));
        self
    }

    /// Register a process manager type to be managed by this store.
    ///
    /// The process manager will be initialized (loading any existing
    /// checkpoint) when [`open`](AggregateStoreBuilder::open) is called.
    /// Use [`run_process_managers`](AggregateStore::run_process_managers)
    /// to trigger catch-up and dispatch.
    ///
    /// # Type Parameters
    ///
    /// * `PM` - A type implementing [`ProcessManager`](crate::process_manager::ProcessManager).
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn process_manager<PM>(mut self) -> Self
    where
        PM: crate::process_manager::ProcessManager,
    {
        self.process_manager_factories.push((
            PM::NAME.to_owned(),
            Box::new(|layout| {
                let runner = ProcessManagerRunner::<PM>::new(layout)?;
                Ok(std::sync::Mutex::new(
                    Box::new(runner) as Box<dyn ProcessManagerCatchUp>
                ))
            }),
        ));
        self
    }

    /// Register an aggregate type as a dispatch target for process managers.
    ///
    /// This allows [`CommandEnvelope`](crate::command::CommandEnvelope)s
    /// targeting this aggregate type to be deserialized and routed. The
    /// aggregate's `Command` type must implement `DeserializeOwned`.
    ///
    /// # Type Parameters
    ///
    /// * `A` - A type implementing [`Aggregate`] with `Command: DeserializeOwned`.
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    /// Set the idle timeout for actor eviction.
    ///
    /// Actors that receive no messages for this duration will shut down,
    /// releasing their file lock. The next [`get`](AggregateStore::get) call
    /// transparently re-spawns the actor and recovers state from disk.
    ///
    /// Defaults to 5 minutes. Pass `Duration::from_secs(u64::MAX / 2)` to
    /// effectively disable idle eviction.
    ///
    /// # Arguments
    ///
    /// * `timeout` - How long an idle actor waits before shutting down.
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Register an aggregate type as a dispatch target for process managers.
    ///
    /// This allows [`CommandEnvelope`](crate::command::CommandEnvelope)s
    /// targeting this aggregate type to be deserialized and routed. The
    /// aggregate's `Command` type must implement `DeserializeOwned`.
    ///
    /// # Type Parameters
    ///
    /// * `A` - A type implementing [`Aggregate`] with `Command: DeserializeOwned`.
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn aggregate_type<A>(mut self) -> Self
    where
        A: Aggregate,
        A::Command: serde::de::DeserializeOwned,
    {
        self.dispatcher_factories.push((
            A::AGGREGATE_TYPE.to_owned(),
            Box::new(|| Box::new(TypedDispatcher::<A>::new()) as Box<dyn AggregateDispatcher>),
        ));
        self
    }

    /// Build and open the store, initializing all registered projections
    /// and process managers.
    ///
    /// Creates the metadata directory on disk and instantiates each
    /// projection runner and process manager runner (loading persisted
    /// checkpoints if available).
    ///
    /// # Returns
    ///
    /// A fully initialized [`AggregateStore`].
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if directory creation or initialization fails.
    pub async fn open(self) -> io::Result<AggregateStore> {
        let layout = StreamLayout::new(&self.base_dir);
        let meta_dir = layout.meta_dir();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(meta_dir))
            .await
            .map_err(io::Error::other)??;

        let mut projections = HashMap::new();
        let mut projection_catch_ups: ProjectionCatchUpList = Vec::new();
        for (name, factory) in self.projection_factories {
            let (any_runner, catch_up) = factory(layout.clone())?;
            projections.insert(name, any_runner);
            projection_catch_ups.push(catch_up);
        }

        let mut process_managers = Vec::new();
        for (_name, factory) in self.process_manager_factories {
            let runner = factory(layout.clone())?;
            process_managers.push(runner);
        }

        let mut dispatchers: HashMap<String, Box<dyn AggregateDispatcher>> = HashMap::new();
        for (name, factory) in self.dispatcher_factories {
            dispatchers.insert(name, factory());
        }

        Ok(AggregateStore {
            layout,
            cache: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(std::sync::RwLock::new(projections)),
            projection_catch_ups: Arc::new(std::sync::RwLock::new(projection_catch_ups)),
            process_managers: Arc::new(std::sync::RwLock::new(process_managers)),
            dispatchers: Arc::new(dispatchers),
            seen_ids: Arc::new(std::sync::Mutex::new(HashSet::new())),
            idle_timeout: self.idle_timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::aggregate::test_fixtures::{Counter, CounterCommand};
    use crate::command::CommandContext;

    #[tokio::test]
    async fn full_roundtrip() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");

        let ctx = CommandContext::default();
        handle
            .execute(CounterCommand::Increment, ctx.clone())
            .await
            .expect("first increment should succeed");
        handle
            .execute(CounterCommand::Increment, ctx)
            .await
            .expect("second increment should succeed");

        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 2);
    }

    #[tokio::test]
    async fn list_empty_initially() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let ids = store.list::<Counter>().await.expect("list should succeed");
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn list_after_commands() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let ctx = CommandContext::default();

        let h1 = store
            .get::<Counter>("c-1")
            .await
            .expect("get c-1 should succeed");
        h1.execute(CounterCommand::Increment, ctx.clone())
            .await
            .expect("c-1 increment should succeed");

        let h2 = store
            .get::<Counter>("c-2")
            .await
            .expect("get c-2 should succeed");
        h2.execute(CounterCommand::Increment, ctx)
            .await
            .expect("c-2 increment should succeed");

        let mut ids = store.list::<Counter>().await.expect("list should succeed");
        ids.sort();
        assert_eq!(ids, vec!["c-1", "c-2"]);
    }

    #[tokio::test]
    async fn same_id_returns_shared_handle() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let h1 = store
            .get::<Counter>("c-1")
            .await
            .expect("first get should succeed");
        let h2 = store
            .get::<Counter>("c-1")
            .await
            .expect("second get should succeed");

        h1.execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment via h1 should succeed");

        let state = h2.state().await.expect("state via h2 should succeed");
        assert_eq!(state.value, 1);
    }

    #[tokio::test]
    async fn state_survives_store_reopen() {
        let tmp = TempDir::new().expect("failed to create temp dir");

        // First store: increment 3 times, then drop everything.
        {
            let store = AggregateStore::open(tmp.path())
                .await
                .expect("open should succeed");
            let handle = store
                .get::<Counter>("c-1")
                .await
                .expect("get should succeed");
            let ctx = CommandContext::default();
            for _ in 0..3 {
                handle
                    .execute(CounterCommand::Increment, ctx.clone())
                    .await
                    .expect("increment should succeed");
            }
        }

        // Brief sleep so the actor thread exits and releases the flock.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Second store on the same directory should recover persisted state.
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("reopen should succeed");
        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get after reopen should succeed");
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 3);
    }

    #[tokio::test]
    async fn two_aggregate_types_coexist() {
        // A minimal second aggregate type for testing type coexistence.
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Toggle {
            pub on: bool,
        }

        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(tag = "type", content = "data")]
        enum ToggleEvent {
            Toggled,
        }

        #[derive(Debug, thiserror::Error)]
        enum ToggleError {}

        impl Aggregate for Toggle {
            const AGGREGATE_TYPE: &'static str = "toggle";
            type Command = ();
            type DomainEvent = ToggleEvent;
            type Error = ToggleError;

            fn handle(&self, _cmd: ()) -> Result<Vec<ToggleEvent>, ToggleError> {
                Ok(vec![ToggleEvent::Toggled])
            }

            fn apply(mut self, _event: &ToggleEvent) -> Self {
                self.on = !self.on;
                self
            }
        }

        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Interact with the Counter aggregate.
        let counter_handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get counter should succeed");
        counter_handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("counter increment should succeed");

        // Interact with the Toggle aggregate.
        let toggle_handle = store
            .get::<Toggle>("t-1")
            .await
            .expect("get toggle should succeed");
        toggle_handle
            .execute((), CommandContext::default())
            .await
            .expect("toggle should succeed");

        // Verify states.
        let counter_state = counter_handle
            .state()
            .await
            .expect("counter state should succeed");
        assert_eq!(counter_state.value, 1);

        let toggle_state = toggle_handle
            .state()
            .await
            .expect("toggle state should succeed");
        assert!(toggle_state.on);

        // Verify listing is type-scoped.
        let counter_ids = store
            .list::<Counter>()
            .await
            .expect("list counters should succeed");
        assert_eq!(counter_ids, vec!["c-1"]);

        let toggle_ids = store
            .list::<Toggle>()
            .await
            .expect("list toggles should succeed");
        assert_eq!(toggle_ids, vec!["t-1"]);
    }

    // --- AggregateStoreBuilder + projection integration tests ---

    use crate::projection::test_fixtures::EventCounter;

    /// Helper: execute a single `Increment` command on the given instance.
    async fn increment(store: &AggregateStore, id: &str) {
        let handle = store.get::<Counter>(id).await.expect("get should succeed");
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment should succeed");
    }

    #[tokio::test]
    async fn builder_with_projection_roundtrip() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .projection::<EventCounter>()
            .open()
            .await
            .expect("builder open should succeed");

        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");
        let ctx = CommandContext::default();
        for _ in 0..3 {
            handle
                .execute(CounterCommand::Increment, ctx.clone())
                .await
                .expect("increment should succeed");
        }

        let counter = store
            .projection::<EventCounter>()
            .expect("projection query should succeed");
        assert_eq!(counter.count, 3);
    }

    #[tokio::test]
    async fn projection_sees_multiple_instances() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .projection::<EventCounter>()
            .open()
            .await
            .expect("builder open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;

        let counter = store
            .projection::<EventCounter>()
            .expect("projection query should succeed");
        assert_eq!(counter.count, 2);
    }

    #[tokio::test]
    async fn projection_persists_across_restart() {
        let tmp = TempDir::new().expect("failed to create temp dir");

        // First store: execute events and query the projection (triggers save).
        {
            let store = AggregateStore::builder(tmp.path())
                .projection::<EventCounter>()
                .open()
                .await
                .expect("builder open should succeed");

            increment(&store, "c-1").await;
            increment(&store, "c-1").await;
            increment(&store, "c-2").await;

            let counter = store
                .projection::<EventCounter>()
                .expect("projection query should succeed");
            assert_eq!(counter.count, 3);
        }

        // Brief sleep so actor threads exit and release flocks.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Second store on the same directory: projection should restore
        // from checkpoint without replaying events.
        let store = AggregateStore::builder(tmp.path())
            .projection::<EventCounter>()
            .open()
            .await
            .expect("reopen should succeed");

        let counter = store
            .projection::<EventCounter>()
            .expect("projection query after reopen should succeed");
        assert_eq!(counter.count, 3);
    }

    #[tokio::test]
    async fn projection_without_registration_returns_error() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let result = store.projection::<EventCounter>();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn open_convenience_still_works() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment should succeed");

        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 1);
    }

    // --- Process manager integration tests ---
    //
    // These tests use a "Receiver" aggregate that accepts deserializable
    // commands, plus a "ForwardSaga" process manager that reacts to Counter
    // events and dispatches commands to the Receiver.

    /// A minimal aggregate that accepts JSON-deserializable commands.
    /// Used as a dispatch target for process manager tests.
    #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Receiver {
        pub received_count: u64,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type", content = "data")]
    enum ReceiverCommand {
        Accept,
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type", content = "data")]
    enum ReceiverEvent {
        Accepted,
    }

    #[derive(Debug, thiserror::Error)]
    enum ReceiverError {}

    impl Aggregate for Receiver {
        const AGGREGATE_TYPE: &'static str = "receiver";
        type Command = ReceiverCommand;
        type DomainEvent = ReceiverEvent;
        type Error = ReceiverError;

        fn handle(&self, _cmd: ReceiverCommand) -> Result<Vec<ReceiverEvent>, ReceiverError> {
            Ok(vec![ReceiverEvent::Accepted])
        }

        fn apply(mut self, _event: &ReceiverEvent) -> Self {
            self.received_count += 1;
            self
        }
    }

    /// A process manager that reacts to counter events by dispatching
    /// `Accept` commands to the Receiver aggregate.
    #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct ForwardSaga {
        pub forwarded: u64,
    }

    impl crate::process_manager::ProcessManager for ForwardSaga {
        const NAME: &'static str = "forward-saga";

        fn subscriptions(&self) -> &'static [&'static str] {
            &["counter"]
        }

        fn react(
            &mut self,
            _aggregate_type: &str,
            stream_id: &str,
            _event: &eventfold::Event,
        ) -> Vec<crate::command::CommandEnvelope> {
            self.forwarded += 1;
            vec![crate::command::CommandEnvelope {
                aggregate_type: "receiver".to_string(),
                instance_id: stream_id.to_string(),
                command: serde_json::json!({"type": "Accept"}),
                context: CommandContext::default(),
            }]
        }
    }

    #[tokio::test]
    async fn end_to_end_process_manager_dispatch() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .process_manager::<ForwardSaga>()
            .aggregate_type::<Receiver>()
            .open()
            .await
            .expect("builder open should succeed");

        // Produce events in the Counter aggregate.
        increment(&store, "c-1").await;
        increment(&store, "c-1").await;

        // Run process managers: should catch up, dispatch to Receiver.
        let report = store
            .run_process_managers()
            .await
            .expect("run_process_managers should succeed");

        assert_eq!(report.dispatched, 2);
        assert_eq!(report.dead_lettered, 0);

        // Verify the Receiver aggregate received the dispatched commands.
        let receiver_handle = store
            .get::<Receiver>("c-1")
            .await
            .expect("get receiver should succeed");
        let receiver_state = receiver_handle
            .state()
            .await
            .expect("receiver state should succeed");
        assert_eq!(receiver_state.received_count, 2);
    }

    #[tokio::test]
    async fn process_manager_dead_letters_unknown_type() {
        /// A process manager that emits commands to a non-existent aggregate.
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        struct BadTargetSaga {
            seen: u64,
        }

        impl crate::process_manager::ProcessManager for BadTargetSaga {
            const NAME: &'static str = "bad-target-saga";

            fn subscriptions(&self) -> &'static [&'static str] {
                &["counter"]
            }

            fn react(
                &mut self,
                _aggregate_type: &str,
                _stream_id: &str,
                _event: &eventfold::Event,
            ) -> Vec<crate::command::CommandEnvelope> {
                self.seen += 1;
                vec![crate::command::CommandEnvelope {
                    aggregate_type: "nonexistent".to_string(),
                    instance_id: "x".to_string(),
                    command: serde_json::json!({}),
                    context: CommandContext::default(),
                }]
            }
        }

        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .process_manager::<BadTargetSaga>()
            .open()
            .await
            .expect("builder open should succeed");

        increment(&store, "c-1").await;

        let report = store
            .run_process_managers()
            .await
            .expect("run_process_managers should succeed");

        assert_eq!(report.dispatched, 0);
        assert_eq!(report.dead_lettered, 1);

        // Verify dead-letter file is readable JSONL.
        let dl_path = tmp
            .path()
            .join("process_managers/bad-target-saga/dead_letters.jsonl");
        let contents = std::fs::read_to_string(&dl_path).expect("dead-letter file should exist");
        let entry: serde_json::Value =
            serde_json::from_str(contents.trim()).expect("dead-letter entry should be valid JSON");
        assert!(
            entry["error"]
                .as_str()
                .expect("error field should be a string")
                .contains("nonexistent")
        );
    }

    #[tokio::test]
    async fn run_process_managers_idempotent() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .process_manager::<ForwardSaga>()
            .aggregate_type::<Receiver>()
            .open()
            .await
            .expect("builder open should succeed");

        increment(&store, "c-1").await;

        // First run: dispatches 1 envelope.
        let first = store
            .run_process_managers()
            .await
            .expect("first run should succeed");
        assert_eq!(first.dispatched, 1);

        // Second run with no new events: should be a no-op.
        let second = store
            .run_process_managers()
            .await
            .expect("second run should succeed");
        assert_eq!(second.dispatched, 0);
        assert_eq!(second.dead_lettered, 0);
    }

    #[tokio::test]
    async fn process_manager_recovers_after_restart() {
        let tmp = TempDir::new().expect("failed to create temp dir");

        // First store: process 2 events.
        {
            let store = AggregateStore::builder(tmp.path())
                .process_manager::<ForwardSaga>()
                .aggregate_type::<Receiver>()
                .open()
                .await
                .expect("builder open should succeed");

            increment(&store, "c-1").await;
            increment(&store, "c-2").await;

            let report = store
                .run_process_managers()
                .await
                .expect("run should succeed");
            assert_eq!(report.dispatched, 2);
        }

        // Brief sleep so actor threads exit.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Second store: add 1 more event, run PM again.
        let store = AggregateStore::builder(tmp.path())
            .process_manager::<ForwardSaga>()
            .aggregate_type::<Receiver>()
            .open()
            .await
            .expect("reopen should succeed");

        increment(&store, "c-1").await;

        let report = store
            .run_process_managers()
            .await
            .expect("run after restart should succeed");

        // Should only dispatch the 1 new event, not replay old ones.
        assert_eq!(report.dispatched, 1);
        assert_eq!(report.dead_lettered, 0);
    }

    // --- Idle eviction tests ---

    #[tokio::test]
    async fn idle_actor_evicted_and_respawned() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .idle_timeout(Duration::from_millis(200))
            .open()
            .await
            .expect("builder open should succeed");

        // Execute a command.
        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");
        handle
            .execute(CounterCommand::Increment, CommandContext::default())
            .await
            .expect("increment should succeed");

        // Wait for the actor to idle out.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            !handle.is_alive(),
            "actor should be dead after idle timeout"
        );

        // A new `get` should transparently re-spawn.
        let handle2 = store
            .get::<Counter>("c-1")
            .await
            .expect("get after eviction should succeed");
        let state = handle2.state().await.expect("state should succeed");
        assert_eq!(state.value, 1, "state should reflect persisted events");
    }

    #[tokio::test]
    async fn rapid_commands_keep_actor_alive() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .idle_timeout(Duration::from_millis(300))
            .open()
            .await
            .expect("builder open should succeed");

        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");

        let ctx = CommandContext::default();
        for _ in 0..5 {
            handle
                .execute(CounterCommand::Increment, ctx.clone())
                .await
                .expect("execute should succeed");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        assert!(
            handle.is_alive(),
            "actor should remain alive during activity"
        );
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 5);
    }

    // --- inject_event tests ---

    /// Helper: build an event matching Counter's "Incremented" variant.
    fn incremented_event() -> eventfold::Event {
        eventfold::Event::new("Incremented", serde_json::Value::Null)
    }

    #[tokio::test]
    async fn inject_event_appends_to_stream() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        store
            .inject_event::<Counter>("c-1", incremented_event(), InjectOptions::default())
            .await
            .expect("inject_event should succeed");

        // Verify the event appears in the JSONL file.
        let jsonl_path = tmp.path().join("streams/counter/c-1/app.jsonl");
        let contents = std::fs::read_to_string(&jsonl_path).expect("app.jsonl should exist");
        assert_eq!(
            contents.lines().count(),
            1,
            "should have exactly one event line"
        );
    }

    #[tokio::test]
    async fn inject_event_projections_reflect_event() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .projection::<EventCounter>()
            .open()
            .await
            .expect("builder open should succeed");

        store
            .inject_event::<Counter>("c-1", incremented_event(), InjectOptions::default())
            .await
            .expect("inject_event should succeed");

        let counter = store
            .projection::<EventCounter>()
            .expect("projection query should succeed");
        assert_eq!(counter.count, 1);
    }

    #[tokio::test]
    async fn inject_event_dedup_by_id() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let event = incremented_event().with_id("ev-1".to_string());

        // First injection should succeed and write.
        store
            .inject_event::<Counter>("c-1", event.clone(), InjectOptions::default())
            .await
            .expect("first inject should succeed");

        // Second injection with the same ID should be a no-op.
        store
            .inject_event::<Counter>("c-1", event, InjectOptions::default())
            .await
            .expect("second inject should succeed (no-op)");

        // Verify only one event in the JSONL.
        let jsonl_path = tmp.path().join("streams/counter/c-1/app.jsonl");
        let contents = std::fs::read_to_string(&jsonl_path).expect("app.jsonl should exist");
        assert_eq!(
            contents.lines().count(),
            1,
            "dedup should prevent second write"
        );
    }

    #[tokio::test]
    async fn inject_event_no_dedup_for_none_id() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Events with id=None should never be deduplicated.
        let event = incremented_event();
        assert!(event.id.is_none(), "precondition: id is None");

        store
            .inject_event::<Counter>("c-1", event.clone(), InjectOptions::default())
            .await
            .expect("first inject should succeed");

        store
            .inject_event::<Counter>("c-1", event, InjectOptions::default())
            .await
            .expect("second inject should succeed");

        let jsonl_path = tmp.path().join("streams/counter/c-1/app.jsonl");
        let contents = std::fs::read_to_string(&jsonl_path).expect("app.jsonl should exist");
        assert_eq!(contents.lines().count(), 2, "both events should be written");
    }

    #[tokio::test]
    async fn inject_options_default_does_not_run_process_managers() {
        let opts = InjectOptions::default();
        assert!(!opts.run_process_managers);
    }

    #[tokio::test]
    async fn inject_event_with_process_managers() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::builder(tmp.path())
            .process_manager::<ForwardSaga>()
            .aggregate_type::<Receiver>()
            .open()
            .await
            .expect("builder open should succeed");

        store
            .inject_event::<Counter>(
                "c-1",
                incremented_event(),
                InjectOptions {
                    run_process_managers: true,
                },
            )
            .await
            .expect("inject_event should succeed");

        // The ForwardSaga should have dispatched a command to Receiver.
        let receiver_handle = store
            .get::<Receiver>("c-1")
            .await
            .expect("get receiver should succeed");
        let receiver_state = receiver_handle
            .state()
            .await
            .expect("receiver state should succeed");
        assert_eq!(
            receiver_state.received_count, 1,
            "process manager should have dispatched"
        );
    }

    #[tokio::test]
    async fn inject_event_with_live_actor() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Spawn an actor for c-1 by getting a handle.
        let handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");
        assert!(handle.is_alive(), "actor should be alive");

        // Inject an event -- should route through the live actor.
        store
            .inject_event::<Counter>("c-1", incremented_event(), InjectOptions::default())
            .await
            .expect("inject_event with live actor should succeed");

        // The actor's view should reflect the injected event.
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 1, "actor should see the injected event");
    }

    #[tokio::test]
    async fn inject_event_creates_new_stream() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Inject into a stream that doesn't exist yet.
        store
            .inject_event::<Counter>(
                "new-instance",
                incremented_event(),
                InjectOptions::default(),
            )
            .await
            .expect("inject_event should create stream");

        // Verify the directory was created.
        let stream_dir = tmp.path().join("streams/counter/new-instance");
        assert!(stream_dir.is_dir(), "stream directory should exist");

        // Verify state via a fresh actor.
        let handle = store
            .get::<Counter>("new-instance")
            .await
            .expect("get should succeed after inject");
        let state = handle.state().await.expect("state should succeed");
        assert_eq!(state.value, 1, "actor should replay the injected event");
    }

    // --- list_streams / read_events tests ---

    // Minimal second aggregate type shared by list_streams tests.
    #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Toggle {
        pub on: bool,
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type", content = "data")]
    enum ToggleEvent {
        Toggled,
    }

    #[derive(Debug, thiserror::Error)]
    enum ToggleError {}

    impl Aggregate for Toggle {
        const AGGREGATE_TYPE: &'static str = "toggle";
        type Command = ();
        type DomainEvent = ToggleEvent;
        type Error = ToggleError;

        fn handle(&self, _cmd: ()) -> Result<Vec<ToggleEvent>, ToggleError> {
            Ok(vec![ToggleEvent::Toggled])
        }

        fn apply(mut self, _event: &ToggleEvent) -> Self {
            self.on = !self.on;
            self
        }
    }

    /// Helper: execute a single Toggle command on the given instance.
    async fn toggle(store: &AggregateStore, id: &str) {
        let handle = store
            .get::<Toggle>(id)
            .await
            .expect("get toggle should succeed");
        handle
            .execute((), CommandContext::default())
            .await
            .expect("toggle should succeed");
    }

    #[tokio::test]
    async fn list_streams_none_returns_all_sorted() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Create counter instances c-1, c-2 and toggle instance t-1.
        increment(&store, "c-1").await;
        increment(&store, "c-2").await;
        toggle(&store, "t-1").await;

        let pairs = store
            .list_streams(None)
            .await
            .expect("list_streams(None) should succeed");

        assert_eq!(
            pairs,
            vec![
                ("counter".to_owned(), "c-1".to_owned()),
                ("counter".to_owned(), "c-2".to_owned()),
                ("toggle".to_owned(), "t-1".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn list_streams_some_filters_by_type() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-2").await;
        toggle(&store, "t-1").await;

        let pairs = store
            .list_streams(Some("counter"))
            .await
            .expect("list_streams(Some) should succeed");

        assert_eq!(
            pairs,
            vec![
                ("counter".to_owned(), "c-1".to_owned()),
                ("counter".to_owned(), "c-2".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn list_streams_none_empty_store() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let pairs = store
            .list_streams(None)
            .await
            .expect("list_streams(None) on empty store should succeed");

        assert!(pairs.is_empty());
    }

    #[tokio::test]
    async fn list_streams_some_nonexistent_type() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let pairs = store
            .list_streams(Some("nonexistent"))
            .await
            .expect("list_streams(Some(nonexistent)) should succeed");

        assert!(pairs.is_empty());
    }

    #[tokio::test]
    async fn read_events_returns_all_events() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        increment(&store, "c-1").await;
        increment(&store, "c-1").await;

        let events = store
            .read_events("counter", "c-1")
            .await
            .expect("read_events should succeed");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "Incremented");
        assert_eq!(events[1].event_type, "Incremented");
    }

    #[tokio::test]
    async fn read_events_empty_stream_returns_empty_vec() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        // Create the stream directory without executing any commands.
        let _handle = store
            .get::<Counter>("c-1")
            .await
            .expect("get should succeed");

        // Drop the handle's actor so the flock is released, then read.
        // The stream directory exists but app.jsonl may or may not exist.
        // In either case read_events should return Ok(vec![]).
        let events = store
            .read_events("counter", "c-1")
            .await
            .expect("read_events on empty stream should succeed");

        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn read_events_nonexistent_stream_returns_not_found() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let result = store.read_events("nonexistent", "x").await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
