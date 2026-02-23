//! Command envelope and dispatch types.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::aggregate::Aggregate;
use crate::error::DispatchError;
use crate::store::AggregateStore;

/// Cross-cutting metadata passed alongside a command.
///
/// Carries audit trail, correlation, and tracing information without
/// polluting the `Command` or `DomainEvent` types. Fields are mapped
/// onto `eventfold::Event` metadata when events are appended.
///
/// # Examples
///
/// ```
/// use eventfold_es::CommandContext;
/// use serde_json::json;
///
/// let ctx = CommandContext::default()
///     .with_actor("user-42")
///     .with_correlation_id("req-abc-123")
///     .with_metadata(json!({"source": "api"}));
///
/// assert_eq!(ctx.actor.as_deref(), Some("user-42"));
/// assert_eq!(ctx.correlation_id.as_deref(), Some("req-abc-123"));
/// assert!(ctx.metadata.is_some());
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandContext {
    /// Identity of the actor issuing the command (e.g. a user ID).
    pub actor: Option<String>,
    /// Correlation ID for tracing a request across aggregates.
    pub correlation_id: Option<String>,
    /// Arbitrary metadata forwarded to `eventfold::Event::meta`.
    pub metadata: Option<Value>,
    /// The device ID of the client that issued the command.
    ///
    /// Stamped into `event.meta["source_device"]` by `to_eventfold_event`.
    /// Skipped during serialization when `None` to maintain backward
    /// compatibility with existing JSONL records.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_device: Option<String>,
}

impl CommandContext {
    /// Set the actor identity.
    ///
    /// # Arguments
    ///
    /// * `actor` - Any value convertible to `String` identifying who issued
    ///   the command (e.g. a user ID or service name).
    ///
    /// # Returns
    ///
    /// The updated `CommandContext` with the actor set.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Set the correlation ID.
    ///
    /// # Arguments
    ///
    /// * `id` - Any value convertible to `String` used to correlate this
    ///   command with other operations across aggregates or services.
    ///
    /// # Returns
    ///
    /// The updated `CommandContext` with the correlation ID set.
    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    /// Set arbitrary metadata.
    ///
    /// # Arguments
    ///
    /// * `meta` - A `serde_json::Value` carrying any additional key-value
    ///   pairs to forward into `eventfold::Event::meta`.
    ///
    /// # Returns
    ///
    /// The updated `CommandContext` with the metadata set.
    pub fn with_metadata(mut self, meta: Value) -> Self {
        self.metadata = Some(meta);
        self
    }

    /// Set the source device ID.
    ///
    /// The device ID identifies which client device originated this
    /// command. It is stamped into `event.meta["source_device"]` by
    /// `to_eventfold_event`, making it available to projections, process
    /// managers, and downstream sync consumers.
    ///
    /// # Arguments
    ///
    /// * `device_id` - Any value convertible to `String` identifying the
    ///   originating device (e.g. a UUID or hostname).
    ///
    /// # Returns
    ///
    /// The updated `CommandContext` with the source device set.
    pub fn with_source_device(mut self, device_id: impl Into<String>) -> Self {
        self.source_device = Some(device_id.into());
        self
    }
}

/// A type-erased command envelope for cross-aggregate dispatch.
///
/// Produced by process managers when reacting to events. The `command` field
/// is a `serde_json::Value` because the process manager does not know the
/// concrete command type of the target aggregate at compile time. The
/// dispatch layer deserializes it into the correct `A::Command` at runtime.
///
/// # Fields
///
/// * `aggregate_type` - Target aggregate type name (must match `Aggregate::AGGREGATE_TYPE`).
/// * `instance_id` - Target aggregate instance identifier.
/// * `command` - JSON-serialized command payload.
/// * `context` - Cross-cutting metadata forwarded to the command handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEnvelope {
    /// Target aggregate type name (must match `Aggregate::AGGREGATE_TYPE`).
    pub aggregate_type: String,
    /// Target aggregate instance identifier.
    pub instance_id: String,
    /// JSON-serialized command payload.
    pub command: Value,
    /// Cross-cutting metadata forwarded to the command handler.
    pub context: CommandContext,
}

// --- CommandBus ---

/// Internal trait for type-erased command routing.
///
/// Each registered aggregate type gets a `TypedCommandRoute<A>` that
/// downcasts the `Box<dyn Any>` command to `A::Command` and dispatches
/// it through the store.
trait CommandRoute: Send + Sync {
    /// Dispatch a type-erased command to the target aggregate instance.
    fn dispatch<'a>(
        &'a self,
        store: &'a AggregateStore,
        instance_id: &'a str,
        cmd: Box<dyn Any + Send>,
        ctx: CommandContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), DispatchError>> + Send + 'a>>;
}

/// Concrete command route for aggregate type `A`.
///
/// Downcasts the `Box<dyn Any>` to `A::Command`, looks up the aggregate
/// handle via `store.get::<A>()`, and executes the command.
struct TypedCommandRoute<A: Aggregate> {
    _marker: std::marker::PhantomData<A>,
}

impl<A: Aggregate> CommandRoute for TypedCommandRoute<A> {
    fn dispatch<'a>(
        &'a self,
        store: &'a AggregateStore,
        instance_id: &'a str,
        cmd: Box<dyn Any + Send>,
        ctx: CommandContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), DispatchError>> + Send + 'a>> {
        Box::pin(async move {
            // Downcast the type-erased command back to the concrete type.
            // This cannot fail when the route was registered correctly via
            // `register::<A>()` because the TypeId lookup guarantees we
            // only arrive here for commands of type `A::Command`.
            let typed_cmd = cmd
                .downcast::<A::Command>()
                .map_err(|_| DispatchError::UnknownCommand)?;

            let handle = store
                .get::<A>(instance_id)
                .await
                .map_err(DispatchError::Io)?;

            handle
                .execute(*typed_cmd, ctx)
                .await
                .map_err(|e| DispatchError::Execution(Box::new(e)))?;

            Ok(())
        })
    }
}

/// Typed command router that maps concrete command types to aggregate handlers.
///
/// Unlike the [`CommandEnvelope`]-based dispatch used by process managers,
/// `CommandBus` routes commands by their Rust `TypeId` at zero runtime cost
/// (single `HashMap` lookup). Commands do **not** need to be serializable.
///
/// # Usage
///
/// ```no_run
/// use eventfold_es::{AggregateStore, CommandBus, CommandContext};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let store = AggregateStore::open("/tmp/my-app").await?;
/// let mut bus = CommandBus::new(store);
/// // bus.register::<MyAggregate>();
/// // bus.dispatch("instance-1", MyCommand { .. }, CommandContext::default()).await?;
/// # Ok(())
/// # }
/// ```
pub struct CommandBus {
    store: AggregateStore,
    routes: HashMap<TypeId, Box<dyn CommandRoute>>,
}

impl CommandBus {
    /// Create a new `CommandBus` backed by the given store.
    ///
    /// # Arguments
    ///
    /// * `store` - The [`AggregateStore`] used to look up and spawn aggregate actors.
    pub fn new(store: AggregateStore) -> Self {
        Self {
            store,
            routes: HashMap::new(),
        }
    }

    /// Register an aggregate type for command routing.
    ///
    /// After registration, commands of type `A::Command` can be dispatched
    /// via [`dispatch`](CommandBus::dispatch). The route is keyed by
    /// `TypeId::of::<A::Command>()`.
    ///
    /// # Type Parameters
    ///
    /// * `A` - An aggregate whose `Command` type will be routed.
    pub fn register<A: Aggregate>(&mut self) {
        let type_id = TypeId::of::<A::Command>();
        self.routes.insert(
            type_id,
            Box::new(TypedCommandRoute::<A> {
                _marker: std::marker::PhantomData,
            }),
        );
    }

    /// Dispatch a command to the appropriate aggregate instance.
    ///
    /// Looks up the route by `TypeId::of::<C>()` and delegates to the
    /// registered aggregate's actor.
    ///
    /// # Arguments
    ///
    /// * `instance_id` - The target aggregate instance identifier.
    /// * `cmd` - The concrete command to dispatch.
    /// * `ctx` - Cross-cutting metadata (actor, correlation ID, etc.).
    ///
    /// # Errors
    ///
    /// * [`DispatchError::UnknownCommand`] -- no aggregate registered for `C`.
    /// * [`DispatchError::Io`] -- actor spawning or I/O failure.
    /// * [`DispatchError::Execution`] -- the aggregate rejected the command.
    pub async fn dispatch<C: Send + 'static>(
        &self,
        instance_id: &str,
        cmd: C,
        ctx: CommandContext,
    ) -> Result<(), DispatchError> {
        let type_id = TypeId::of::<C>();
        let route = self
            .routes
            .get(&type_id)
            .ok_or(DispatchError::UnknownCommand)?;
        route
            .dispatch(&self.store, instance_id, Box::new(cmd), ctx)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_context_has_no_fields_set() {
        let ctx = CommandContext::default();
        assert_eq!(ctx.actor, None);
        assert_eq!(ctx.correlation_id, None);
        assert_eq!(ctx.metadata, None);
        assert_eq!(ctx.source_device, None);
    }

    #[test]
    fn builder_sets_actor() {
        let ctx = CommandContext::default().with_actor("user-1");
        assert_eq!(ctx.actor.as_deref(), Some("user-1"));
    }

    #[test]
    fn builder_sets_correlation_id() {
        let ctx = CommandContext::default().with_correlation_id("corr-99");
        assert_eq!(ctx.correlation_id.as_deref(), Some("corr-99"));
    }

    #[test]
    fn builder_sets_metadata() {
        let meta = json!({"key": "value"});
        let ctx = CommandContext::default().with_metadata(meta.clone());
        assert_eq!(ctx.metadata, Some(meta));
    }

    #[test]
    fn builder_chains_all_fields() {
        let ctx = CommandContext::default()
            .with_actor("admin")
            .with_correlation_id("req-abc")
            .with_metadata(json!({"source": "test"}))
            .with_source_device("phone-42");

        assert_eq!(ctx.actor.as_deref(), Some("admin"));
        assert_eq!(ctx.correlation_id.as_deref(), Some("req-abc"));
        assert_eq!(ctx.metadata, Some(json!({"source": "test"})));
        assert_eq!(ctx.source_device.as_deref(), Some("phone-42"));
    }

    #[test]
    fn builder_sets_source_device() {
        let ctx = CommandContext::default().with_source_device("device-abc");
        assert_eq!(ctx.source_device.as_deref(), Some("device-abc"));
    }

    #[test]
    fn builder_accepts_string_owned() {
        // Verify `impl Into<String>` works with owned `String` values,
        // not just `&str` literals.
        let ctx = CommandContext::default()
            .with_actor(String::from("svc-payments"))
            .with_correlation_id(String::from("id-007"))
            .with_source_device(String::from("laptop-01"));

        assert_eq!(ctx.actor.as_deref(), Some("svc-payments"));
        assert_eq!(ctx.correlation_id.as_deref(), Some("id-007"));
        assert_eq!(ctx.source_device.as_deref(), Some("laptop-01"));
    }

    #[test]
    fn clone_produces_independent_copy() {
        let original = CommandContext::default()
            .with_actor("user-1")
            .with_metadata(json!({"a": 1}));

        let cloned = original.clone();
        assert_eq!(original.actor, cloned.actor);
        assert_eq!(original.metadata, cloned.metadata);
    }

    #[test]
    fn debug_format_is_readable() {
        let ctx = CommandContext::default().with_actor("dbg-user");
        let debug_output = format!("{ctx:?}");
        assert!(debug_output.contains("dbg-user"));
    }

    #[test]
    fn command_context_serde_roundtrip() {
        let ctx = CommandContext::default()
            .with_actor("user-1")
            .with_correlation_id("corr-1")
            .with_metadata(json!({"key": "value"}))
            .with_source_device("device-xyz");

        let json = serde_json::to_string(&ctx).expect("serialization should succeed");
        let deserialized: CommandContext =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.actor, ctx.actor);
        assert_eq!(deserialized.correlation_id, ctx.correlation_id);
        assert_eq!(deserialized.metadata, ctx.metadata);
        assert_eq!(deserialized.source_device, ctx.source_device);
    }

    #[test]
    fn source_device_none_omitted_from_json() {
        // When source_device is None, the key must not appear in JSON output.
        let ctx = CommandContext::default().with_actor("user-1");
        let json = serde_json::to_string(&ctx).expect("serialization should succeed");
        assert!(
            !json.contains("source_device"),
            "source_device key should be absent when None, got: {json}"
        );
    }

    #[test]
    fn deserialize_legacy_json_without_source_device() {
        // Simulates a JSON record produced by a prior version of the library
        // that has no source_device key. Must deserialize with source_device = None.
        let legacy_json = r#"{"actor":"old-user","correlation_id":"old-corr","metadata":null}"#;
        let ctx: CommandContext =
            serde_json::from_str(legacy_json).expect("deserialization should succeed");
        assert_eq!(ctx.actor.as_deref(), Some("old-user"));
        assert_eq!(ctx.source_device, None);
    }

    #[test]
    fn command_envelope_serde_roundtrip() {
        let envelope = CommandEnvelope {
            aggregate_type: "counter".to_string(),
            instance_id: "c-1".to_string(),
            command: json!({"type": "Increment"}),
            context: CommandContext::default().with_actor("saga"),
        };

        let json = serde_json::to_string(&envelope).expect("serialization should succeed");
        let deserialized: CommandEnvelope =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.aggregate_type, envelope.aggregate_type);
        assert_eq!(deserialized.instance_id, envelope.instance_id);
        assert_eq!(deserialized.command, envelope.command);
        assert_eq!(deserialized.context.actor, envelope.context.actor);
    }

    // --- CommandBus tests ---

    use tempfile::TempDir;

    use crate::aggregate::test_fixtures::{Counter, CounterCommand};
    use crate::error::DispatchError;
    use crate::store::AggregateStore;

    // A second aggregate type for testing multi-type dispatch.
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

    /// Command type for Toggle -- a unit struct to avoid TypeId collision
    /// with the `()` unit type.
    struct ToggleCmd;

    impl crate::aggregate::Aggregate for Toggle {
        const AGGREGATE_TYPE: &'static str = "toggle";
        type Command = ToggleCmd;
        type DomainEvent = ToggleEvent;
        type Error = ToggleError;

        fn handle(&self, _cmd: ToggleCmd) -> Result<Vec<ToggleEvent>, ToggleError> {
            Ok(vec![ToggleEvent::Toggled])
        }

        fn apply(mut self, _event: &ToggleEvent) -> Self {
            self.on = !self.on;
            self
        }
    }

    #[tokio::test]
    async fn command_bus_dispatch_to_two_aggregate_types() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let mut bus = CommandBus::new(store.clone());
        bus.register::<Counter>();
        bus.register::<Toggle>();

        // Dispatch to Counter.
        bus.dispatch("c-1", CounterCommand::Increment, CommandContext::default())
            .await
            .expect("counter dispatch should succeed");
        bus.dispatch("c-1", CounterCommand::Increment, CommandContext::default())
            .await
            .expect("second counter dispatch should succeed");

        // Dispatch to Toggle.
        bus.dispatch("t-1", ToggleCmd, CommandContext::default())
            .await
            .expect("toggle dispatch should succeed");

        // Verify state through the store.
        let counter_state = store
            .get::<Counter>("c-1")
            .await
            .expect("get counter should succeed")
            .state()
            .await
            .expect("counter state should succeed");
        assert_eq!(counter_state.value, 2);

        let toggle_state = store
            .get::<Toggle>("t-1")
            .await
            .expect("get toggle should succeed")
            .state()
            .await
            .expect("toggle state should succeed");
        assert!(toggle_state.on);
    }

    #[tokio::test]
    async fn command_bus_unknown_command_returns_error() {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let store = AggregateStore::open(tmp.path())
            .await
            .expect("open should succeed");

        let bus = CommandBus::new(store);
        // No types registered.

        let result = bus
            .dispatch("c-1", CounterCommand::Increment, CommandContext::default())
            .await;

        assert!(
            matches!(result, Err(DispatchError::UnknownCommand)),
            "expected UnknownCommand, got: {result:?}"
        );
    }
}
