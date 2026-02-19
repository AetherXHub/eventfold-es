//! Top-level entry point that composes storage layout, actor spawning, and
//! handle caching into a single `AggregateStore` type.

use std::any::Any;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::actor::{AggregateHandle, spawn_actor};
use crate::aggregate::Aggregate;
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
            {
                return Ok(handle.clone());
            }
        }

        // Slow path: create stream directory and spawn actor.
        let layout = self.layout.clone();
        let agg_type = A::AGGREGATE_TYPE.to_owned();
        let inst_id = id.to_owned();
        let stream_dir =
            tokio::task::spawn_blocking(move || layout.ensure_stream(&agg_type, &inst_id))
                .await
                .map_err(io::Error::other)??;

        let handle = spawn_actor::<A>(&stream_dir)?;

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
        // `Mutex<ProjectionRunner<P>>`. This is safe because `projection()`
        // on the builder registered the runner under the same `P::NAME` key.
        let runner_mutex = runner_any
            .downcast_ref::<std::sync::Mutex<ProjectionRunner<P>>>()
            .ok_or_else(|| io::Error::other("projection type mismatch"))?;
        let mut runner = runner_mutex
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        runner.catch_up()?;
        Ok(runner.state().clone())
    }

    /// Returns a reference to the underlying storage layout.
    pub fn layout(&self) -> &StreamLayout {
        &self.layout
    }
}

/// Factory function type for creating a type-erased projection runner.
///
/// Each closure captures the concrete `P: Projection` type, creates a
/// `ProjectionRunner<P>`, wraps it in `std::sync::Mutex`, and returns it
/// as `Box<dyn Any + Send + Sync>` for storage in the projection map.
type ProjectionFactory = Box<dyn FnOnce(StreamLayout) -> io::Result<Box<dyn Any + Send + Sync>>>;

/// Builder for configuring an [`AggregateStore`] with projections.
///
/// Created via [`AggregateStore::builder`]. Register projections with
/// [`projection`](AggregateStoreBuilder::projection), then call
/// [`open`](AggregateStoreBuilder::open) to finalize.
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
pub struct AggregateStoreBuilder {
    base_dir: PathBuf,
    projection_factories: Vec<(String, ProjectionFactory)>,
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
                Ok(Box::new(std::sync::Mutex::new(runner)) as Box<dyn Any + Send + Sync>)
            }),
        ));
        self
    }

    /// Build and open the store, initializing all registered projections.
    ///
    /// Creates the metadata directory on disk and instantiates each
    /// projection runner (loading persisted checkpoints if available).
    ///
    /// # Returns
    ///
    /// A fully initialized [`AggregateStore`].
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if directory creation or projection initialization fails.
    pub async fn open(self) -> io::Result<AggregateStore> {
        let layout = StreamLayout::new(&self.base_dir);
        let meta_dir = layout.meta_dir();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(meta_dir))
            .await
            .map_err(io::Error::other)??;

        let mut projections = HashMap::new();
        for (name, factory) in self.projection_factories {
            let runner = factory(layout.clone())?;
            projections.insert(name, runner);
        }

        Ok(AggregateStore {
            layout,
            cache: Arc::new(RwLock::new(HashMap::new())),
            projections: Arc::new(std::sync::RwLock::new(projections)),
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
}
