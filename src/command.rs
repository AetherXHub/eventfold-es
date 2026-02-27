//! Command envelope and dispatch types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cross-cutting metadata passed alongside a command.
///
/// Carries audit trail, correlation, and tracing information without
/// polluting the `Command` or `DomainEvent` types. Fields are mapped
/// onto event metadata when events are appended.
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
    /// Arbitrary metadata forwarded to event metadata.
    pub metadata: Option<Value>,
    /// The device ID of the client that issued the command.
    ///
    /// Stamped into event metadata as `source_device`.
    /// Skipped during serialization when `None` to maintain backward
    /// compatibility with existing records.
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
    ///   pairs to forward into event metadata.
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
    /// command. It is stamped into event metadata as `source_device`,
    /// making it available to projections, process managers, and
    /// downstream sync consumers.
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
        let ctx = CommandContext::default().with_actor("user-1");
        let json = serde_json::to_string(&ctx).expect("serialization should succeed");
        assert!(
            !json.contains("source_device"),
            "source_device key should be absent when None, got: {json}"
        );
    }

    #[test]
    fn deserialize_legacy_json_without_source_device() {
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
}
