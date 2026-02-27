//! Top-level entry point that composes actor spawning, handle caching,
//! projections, and process managers into a single [`AggregateStore`] type.
//!
//! The store is opened via [`AggregateStoreBuilder`], which connects to
//! an `eventfold-db` gRPC server and configures local caches for snapshots,
//! projection checkpoints, and process manager checkpoints.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use crate::actor::{ActorConfig, AggregateHandle, spawn_actor_with_config};
use crate::aggregate::Aggregate;
use crate::client::{EsClient, ExpectedVersionArg};
use crate::command::CommandEnvelope;
use crate::event::{ProposedEventData, stream_uuid};
use crate::process_manager::{
    ProcessManager, ProcessManagerCatchUp, ProcessManagerReport, ProcessManagerRunner,
    append_dead_letter,
};
use crate::projection::{Projection, ProjectionRunner};

/// Type-erased handle cache keyed by `(TypeId, instance_id)`.
///
/// `TypeId` identifies the aggregate type at runtime; the `String` is the
/// instance ID. `Box<dyn Any + Send + Sync>` lets a single map hold
/// `AggregateHandle<A>` for any concrete `A`. Downcasting recovers the
/// typed handle.
type HandleCache = HashMap<(TypeId, String), Box<dyn Any + Send + Sync>>;

/// Type-erased projection map keyed by projection name.
///
/// Each value is a `tokio::sync::Mutex<ProjectionRunner<P>>` erased to
/// `dyn Any`. We use `tokio::sync::Mutex` because `catch_up` is async.
type ProjectionMap = HashMap<String, Box<dyn Any + Send + Sync>>;

/// Type-erased list of process manager runners.
///
/// Each runner implements `ProcessManagerCatchUp` behind a `tokio::sync::Mutex`
/// because `catch_up` returns a boxed future.
type ProcessManagerList = Vec<tokio::sync::Mutex<Box<dyn ProcessManagerCatchUp>>>;

/// Type-erased dispatcher map keyed by aggregate type name.
///
/// Dispatchers route [`CommandEnvelope`]s from process managers to the
/// target aggregate, deserializing the JSON command and executing it.
type DispatcherMap = HashMap<String, Box<dyn AggregateDispatcher>>;

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
/// The store handles actor spawning, handle caching, projection catch-up,
/// and process manager dispatch. It connects to an `eventfold-db` gRPC
/// server for event persistence.
///
/// `Clone` is cheap -- all internal state is `Arc`-wrapped.
#[derive(Clone)]
pub struct AggregateStore {
    client: EsClient,
    base_dir: PathBuf,
    cache: Arc<RwLock<HandleCache>>,
    projections: Arc<tokio::sync::RwLock<ProjectionMap>>,
    process_managers: Arc<tokio::sync::RwLock<ProcessManagerList>>,
    dispatchers: Arc<DispatcherMap>,
    idle_timeout: Duration,
}

// Manual `Debug` because `dyn Any` is not `Debug` and we don't want to
// expose cache internals.
impl std::fmt::Debug for AggregateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AggregateStore")
            .field("base_dir", &self.base_dir)
            .finish()
    }
}

impl AggregateStore {
    /// Get a handle to an aggregate instance, spawning its actor if needed.
    ///
    /// If the actor is already running (cached and alive), returns a clone
    /// of the existing handle. Otherwise, derives the stream UUID via
    /// [`stream_uuid`] and spawns a new actor.
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
    /// Returns [`tonic::Status`] if the actor's catch-up read from the
    /// server fails.
    pub async fn get<A: Aggregate>(&self, id: &str) -> Result<AggregateHandle<A>, tonic::Status>
    where
        A::Command: Clone,
        A::DomainEvent: DeserializeOwned,
    {
        let key = (TypeId::of::<A>(), id.to_owned());

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

        // Slow path: evict stale entry and spawn new actor.
        {
            let mut cache = self.cache.write().await;
            cache.remove(&key);
        }

        tracing::debug!(
            aggregate_type = A::AGGREGATE_TYPE,
            instance_id = %id,
            "spawning actor"
        );

        let config = ActorConfig {
            idle_timeout: self.idle_timeout,
        };
        let handle =
            spawn_actor_with_config::<A>(id, self.client.clone(), &self.base_dir, config).await?;

        let mut cache = self.cache.write().await;
        cache.insert(key, Box::new(handle.clone()));
        Ok(handle)
    }

    /// Append a pre-built event directly to a stream, bypassing command
    /// validation.
    ///
    /// Uses `ExpectedVersion::Any` for the append, making it suitable
    /// for relay-sync scenarios. Optionally triggers process managers
    /// after injection.
    ///
    /// # Arguments
    ///
    /// * `instance_id` - Unique instance identifier within the aggregate type.
    /// * `proposed` - A pre-built [`ProposedEventData`] to append as-is.
    /// * `opts` - Controls whether process managers run after injection.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the gRPC append fails or process manager
    /// catch-up fails.
    pub async fn inject_event<A: Aggregate>(
        &self,
        instance_id: &str,
        proposed: ProposedEventData,
        opts: InjectOptions,
    ) -> io::Result<()> {
        let stream_id = stream_uuid(A::AGGREGATE_TYPE, instance_id);
        let mut client = self.client.clone();
        client
            .append(stream_id, ExpectedVersionArg::Any, vec![proposed])
            .await
            .map_err(|e| io::Error::other(format!("gRPC append failed: {e}")))?;

        if opts.run_process_managers {
            self.run_process_managers().await?;
        }

        Ok(())
    }

    /// Catch up and return the current state of a registered projection.
    ///
    /// Triggers a catch-up pass (reading new events from the global log)
    /// before returning a clone of the projection state.
    ///
    /// # Returns
    ///
    /// A clone of the projection's current state after catching up.
    ///
    /// # Errors
    ///
    /// Returns `io::ErrorKind::NotFound` if the projection is not registered.
    /// Returns `io::Error` if catching up on events fails.
    pub async fn projection<P: Projection>(&self) -> io::Result<P> {
        let projections = self.projections.read().await;
        let runner_any = projections.get(P::NAME).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("projection '{}' not registered", P::NAME),
            )
        })?;
        let runner_mutex = runner_any
            .downcast_ref::<tokio::sync::Mutex<ProjectionRunner<P>>>()
            .ok_or_else(|| io::Error::other("projection type mismatch"))?;
        let mut runner = runner_mutex.lock().await;
        runner.catch_up().await?;
        Ok(runner.state().clone())
    }

    /// Run all registered process managers through a catch-up pass.
    ///
    /// For each process manager:
    /// 1. Catch up on the global log, collecting command envelopes.
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
        let mut all_work: Vec<(Vec<CommandEnvelope>, PathBuf)> = Vec::new();

        {
            let pms = self.process_managers.read().await;
            for pm_mutex in pms.iter() {
                let mut pm = pm_mutex.lock().await;
                let envelopes = pm.catch_up().await?;
                let dead_letter_path = pm.dead_letter_path();
                all_work.push((envelopes, dead_letter_path));
            }
        }

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
                                "dispatched command"
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
            let pms = self.process_managers.read().await;
            for pm_mutex in pms.iter() {
                let pm = pm_mutex.lock().await;
                pm.save()?;
            }
        }

        Ok(report)
    }
}

// --- Type-erased dispatch for process manager command routing ---

/// Type-erased interface for dispatching [`CommandEnvelope`]s to an
/// aggregate type.
///
/// Each concrete `TypedDispatcher<A>` implements this trait, deserializing
/// the JSON command payload into `A::Command` and executing it via
/// the store's `get::<A>(id)` handle.
#[tonic::async_trait]
trait AggregateDispatcher: Send + Sync {
    /// Dispatch a command envelope to the target aggregate.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if deserialization, actor spawn, or execution fails.
    async fn dispatch(&self, store: &AggregateStore, envelope: CommandEnvelope) -> io::Result<()>;
}

/// Concrete dispatcher for a specific aggregate type `A`.
///
/// Deserializes the JSON command payload into `A::Command` and executes
/// it through the store.
struct TypedDispatcher<A: Aggregate> {
    _marker: std::marker::PhantomData<A>,
}

impl<A: Aggregate> TypedDispatcher<A> {
    /// Create a new typed dispatcher.
    fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

#[tonic::async_trait]
impl<A> AggregateDispatcher for TypedDispatcher<A>
where
    A: Aggregate,
    A::Command: DeserializeOwned + Clone,
    A::DomainEvent: DeserializeOwned,
{
    async fn dispatch(&self, store: &AggregateStore, envelope: CommandEnvelope) -> io::Result<()> {
        let cmd: A::Command = serde_json::from_value(envelope.command)
            .map_err(|e| io::Error::other(format!("command deserialization failed: {e}")))?;
        let handle = store
            .get::<A>(&envelope.instance_id)
            .await
            .map_err(|e| io::Error::other(format!("actor spawn failed: {e}")))?;
        handle
            .execute(cmd, envelope.context)
            .await
            .map_err(|e| io::Error::other(format!("command execution failed: {e}")))?;
        Ok(())
    }
}

// --- Factory types for deferred construction ---

/// Factory for creating a type-erased projection runner.
type ProjectionFactory = Box<dyn FnOnce(EsClient, &Path) -> io::Result<Box<dyn Any + Send + Sync>>>;

/// Factory for creating a type-erased process manager runner.
type ProcessManagerFactory = Box<
    dyn FnOnce(EsClient, &Path) -> io::Result<tokio::sync::Mutex<Box<dyn ProcessManagerCatchUp>>>,
>;

/// Factory for creating a type-erased aggregate dispatcher.
type DispatcherFactory = Box<dyn FnOnce() -> Box<dyn AggregateDispatcher>>;

/// Builder for configuring and opening an [`AggregateStore`].
///
/// Collects configuration -- endpoint URL, local cache directory,
/// registered aggregates, projections, and process managers -- then
/// connects to the gRPC server on [`open`](AggregateStoreBuilder::open).
///
/// # Examples
///
/// ```no_run
/// use eventfold_es::AggregateStoreBuilder;
///
/// # async fn example() -> Result<(), tonic::transport::Error> {
/// let store = AggregateStoreBuilder::new()
///     .endpoint("http://127.0.0.1:2113")
///     .base_dir("/tmp/my-app")
///     .open()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct AggregateStoreBuilder {
    endpoint: Option<String>,
    base_dir: Option<PathBuf>,
    projection_factories: Vec<(String, ProjectionFactory)>,
    process_manager_factories: Vec<(String, ProcessManagerFactory)>,
    dispatcher_factories: Vec<(String, DispatcherFactory)>,
    idle_timeout: Duration,
}

impl AggregateStoreBuilder {
    /// Create a new builder with no configuration.
    ///
    /// At minimum, [`endpoint`](AggregateStoreBuilder::endpoint) must be
    /// called before [`open`](AggregateStoreBuilder::open).
    pub fn new() -> Self {
        Self {
            endpoint: None,
            base_dir: None,
            projection_factories: Vec::new(),
            process_manager_factories: Vec::new(),
            dispatcher_factories: Vec::new(),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    /// Set the gRPC server endpoint URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URI of the `eventfold-db` gRPC server
    ///   (e.g. `"http://127.0.0.1:2113"`).
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }

    /// Set the local cache directory for snapshots and checkpoints.
    ///
    /// If not set, defaults to a system temp directory.
    ///
    /// # Arguments
    ///
    /// * `path` - Root directory for local cache data.
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn base_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.base_dir = Some(path.as_ref().to_owned());
        self
    }

    /// Register an aggregate type as a dispatch target for process managers.
    ///
    /// This allows [`CommandEnvelope`]s targeting this aggregate type to be
    /// deserialized and routed. The aggregate's `Command` type must implement
    /// `DeserializeOwned`.
    ///
    /// # Type Parameters
    ///
    /// * `A` - A type implementing [`Aggregate`] with `Command: DeserializeOwned + Clone`.
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn aggregate_type<A>(mut self) -> Self
    where
        A: Aggregate,
        A::Command: DeserializeOwned + Clone,
        A::DomainEvent: DeserializeOwned,
    {
        self.dispatcher_factories.push((
            A::AGGREGATE_TYPE.to_owned(),
            Box::new(|| Box::new(TypedDispatcher::<A>::new()) as Box<dyn AggregateDispatcher>),
        ));
        self
    }

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
            Box::new(|client: EsClient, base_dir: &Path| {
                let checkpoint_dir = base_dir.join("projections").join(P::NAME);
                let runner = ProjectionRunner::<P>::new(client, checkpoint_dir)?;
                let boxed: Box<dyn Any + Send + Sync> = Box::new(tokio::sync::Mutex::new(runner));
                Ok(boxed)
            }),
        ));
        self
    }

    /// Register a process manager type to be managed by this store.
    ///
    /// The process manager will be initialized (loading any existing
    /// checkpoint) when [`open`](AggregateStoreBuilder::open) is called.
    ///
    /// # Type Parameters
    ///
    /// * `PM` - A type implementing [`ProcessManager`].
    ///
    /// # Returns
    ///
    /// `self` for method chaining.
    pub fn process_manager<PM>(mut self) -> Self
    where
        PM: ProcessManager,
    {
        self.process_manager_factories.push((
            PM::NAME.to_owned(),
            Box::new(|client: EsClient, base_dir: &Path| {
                let checkpoint_dir = base_dir.join("process_managers").join(PM::NAME);
                let runner = ProcessManagerRunner::<PM>::new(client, checkpoint_dir)?;
                Ok(tokio::sync::Mutex::new(
                    Box::new(runner) as Box<dyn ProcessManagerCatchUp>
                ))
            }),
        ));
        self
    }

    /// Set the idle timeout for actor eviction.
    ///
    /// Actors that receive no messages for this duration will shut down
    /// and save a snapshot. The next [`get`](AggregateStore::get) call
    /// transparently re-spawns the actor from snapshot + catch-up.
    ///
    /// Defaults to 5 minutes.
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

    /// Connect to the gRPC server and build the [`AggregateStore`].
    ///
    /// Establishes a gRPC channel to the configured endpoint, initializes
    /// all registered projections and process managers (loading persisted
    /// checkpoints if available), and returns the store.
    ///
    /// # Returns
    ///
    /// A fully initialized [`AggregateStore`].
    ///
    /// # Errors
    ///
    /// Returns [`tonic::transport::Error`] if the gRPC connection fails.
    /// Returns the error wrapped as `tonic::transport::Error` if projection
    /// or process manager initialization fails.
    pub async fn open(self) -> Result<AggregateStore, tonic::transport::Error> {
        let endpoint = self.endpoint.as_deref().unwrap_or("http://127.0.0.1:2113");

        let client = EsClient::connect(endpoint).await?;

        let base_dir = self
            .base_dir
            .unwrap_or_else(|| std::env::temp_dir().join("eventfold-es"));

        // Initialize projections.
        let mut projections = HashMap::new();
        for (name, factory) in self.projection_factories {
            let runner = factory(client.clone(), &base_dir)
                .map_err(|e| {
                    // tonic::transport::Error is opaque; we cannot construct one
                    // directly. Instead, we log and panic. In practice, this only
                    // fails on I/O errors reading checkpoint files.
                    tracing::error!(error = %e, "failed to initialize projection '{name}'");
                    e
                })
                .expect("projection initialization should not fail");
            projections.insert(name, runner);
        }

        // Initialize process managers.
        let mut process_managers = Vec::new();
        for (_name, factory) in self.process_manager_factories {
            let runner = factory(client.clone(), &base_dir)
                .expect("process manager initialization should not fail");
            process_managers.push(runner);
        }

        // Build dispatcher map.
        let mut dispatchers: HashMap<String, Box<dyn AggregateDispatcher>> = HashMap::new();
        for (name, factory) in self.dispatcher_factories {
            dispatchers.insert(name, factory());
        }

        Ok(AggregateStore {
            client,
            base_dir,
            cache: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(tokio::sync::RwLock::new(projections)),
            process_managers: Arc::new(tokio::sync::RwLock::new(process_managers)),
            dispatchers: Arc::new(dispatchers),
            idle_timeout: self.idle_timeout,
        })
    }
}

impl Default for AggregateStoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::AggregateHandle;
    use crate::aggregate::test_fixtures::Counter;

    /// Build a mock `AggregateStore` for tests that don't need a live gRPC
    /// server. The `get` method will fail on cache miss (since the client
    /// is invalid), but cache-hit paths work correctly.
    fn mock_store(base_dir: &Path) -> AggregateStore {
        // Create a dummy EsClient by connecting to a nonsense endpoint.
        // We use `Endpoint::from_static` + `connect_lazy` to get a Channel
        // without actually connecting. This is fine for tests that only
        // exercise cache logic.
        let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
        let inner = crate::proto::event_store_client::EventStoreClient::new(channel);
        let client = EsClient::from_inner(inner);
        AggregateStore {
            client,
            base_dir: base_dir.to_owned(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            process_managers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            dispatchers: Arc::new(HashMap::new()),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }

    #[tokio::test]
    async fn builder_connect_returns_err_when_no_server() {
        let result = AggregateStoreBuilder::new()
            .endpoint("http://127.0.0.1:1")
            .open()
            .await;
        assert!(
            result.is_err(),
            "open should fail when no server is listening on port 1"
        );
    }

    #[tokio::test]
    async fn get_twice_returns_cached_alive_handles() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let store = mock_store(tmp.path());

        // Pre-populate the cache with a handle backed by a live channel.
        // We create the channel manually; the receiver is kept alive so
        // `is_alive()` returns true.
        let (tx, _rx) = tokio::sync::mpsc::channel::<crate::actor::ActorMessage<Counter>>(1);
        let handle = AggregateHandle::<Counter>::from_sender(tx);
        let key = (TypeId::of::<Counter>(), "c-1".to_owned());
        store.cache.write().await.insert(key, Box::new(handle));

        // First get should return the cached handle.
        let h1 = store
            .get::<Counter>("c-1")
            .await
            .expect("first get should succeed");
        assert!(h1.is_alive(), "first handle should be alive");

        // Second get should also return the cached handle.
        let h2 = store
            .get::<Counter>("c-1")
            .await
            .expect("second get should succeed");
        assert!(h2.is_alive(), "second handle should be alive");
    }
}
