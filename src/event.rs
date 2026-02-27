//! Event encoding, decoding, and shared types for the gRPC event store backend.
//!
//! This module provides the foundational data types and pure functions that
//! the actor, projection, and process manager modules all depend on. No
//! network I/O occurs here.

use uuid::Uuid;

/// Fixed namespace UUID for deterministic stream ID derivation.
///
/// All `eventfold-es` stream IDs are UUID v5 values derived from this
/// namespace and the `"{aggregate_type}/{instance_id}"` string. This
/// ensures the same aggregate identity always maps to the same stream
/// UUID, regardless of which process performs the mapping.
const STREAM_NAMESPACE: Uuid = Uuid::from_bytes([
    0x9a, 0x1e, 0x7c, 0x3b, 0x4d, 0x2f, 0x4a, 0x8e, 0xb5, 0x6c, 0x1f, 0x3d, 0x7e, 0x9a, 0x0b, 0xc4,
]);

/// Derive a deterministic stream UUID from aggregate type and instance ID.
///
/// Uses UUID v5 (SHA-1 based) with [`STREAM_NAMESPACE`] to produce
/// a stable, collision-resistant stream identifier suitable for use
/// as an `eventfold-db` stream ID.
///
/// # Arguments
///
/// * `aggregate_type` - The aggregate type name (e.g., "counter").
/// * `instance_id` - The aggregate instance identifier (e.g., "c-1").
///
/// # Returns
///
/// A deterministic UUID v5 derived from the concatenation
/// `"{aggregate_type}/{instance_id}"`.
///
/// # Examples
///
/// ```
/// use eventfold_es::stream_uuid;
/// let id = stream_uuid("counter", "c-1");
/// assert_eq!(id, stream_uuid("counter", "c-1")); // deterministic
/// ```
pub fn stream_uuid(aggregate_type: &str, instance_id: &str) -> Uuid {
    let name = format!("{aggregate_type}/{instance_id}");
    Uuid::new_v5(&STREAM_NAMESPACE, name.as_bytes())
}

use crate::aggregate::Aggregate;
use crate::command::CommandContext;

/// Infrastructure metadata stamped on every event written by `eventfold-es`.
///
/// Carried in the `metadata` bytes field of the gRPC `ProposedEvent` and
/// `RecordedEvent`. The `aggregate_type` and `instance_id` fields make
/// each event self-describing, so projections and process managers can
/// recover the aggregate identity without an external registry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventMetadata {
    /// Aggregate type name (e.g., "counter").
    pub aggregate_type: String,
    /// Aggregate instance identifier (e.g., "c-1").
    pub instance_id: String,
    /// Actor identity from the command context, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    /// Correlation ID from the command context, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Source device ID from the command context, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_device: Option<String>,
}

/// Rust-native intermediate representation of a proposed event.
///
/// Produced by [`encode_domain_event`] before the event is serialized
/// into the gRPC `ProposedEvent` proto message. Contains typed fields
/// so callers can inspect or transform the event before sending.
#[derive(Debug, Clone)]
pub struct ProposedEventData {
    /// Newly generated UUID v4 event ID.
    pub event_id: Uuid,
    /// Event type tag extracted from the adjacently-tagged domain event.
    pub event_type: String,
    /// JSON payload (the `"data"` portion of the adjacently-tagged enum).
    pub payload: serde_json::Value,
    /// Infrastructure metadata to stamp on the event.
    pub metadata: EventMetadata,
}

/// Encode a domain event into a [`ProposedEventData`] ready for gRPC submission.
///
/// Serializes the adjacently-tagged domain event (`#[serde(tag = "type", content = "data")]`),
/// extracts the `"type"` and `"data"` fields, builds [`EventMetadata`] from the
/// command context and aggregate identity, and generates a fresh UUID v4 event ID.
///
/// # Arguments
///
/// * `event` - Reference to the domain event to encode.
/// * `ctx` - Command context carrying actor, correlation ID, and source device.
/// * `aggregate_type` - The aggregate type name (e.g., "counter").
/// * `instance_id` - The aggregate instance identifier (e.g., "c-1").
///
/// # Returns
///
/// A [`ProposedEventData`] with all fields populated.
///
/// # Errors
///
/// Returns `serde_json::Error` if the domain event cannot be serialized to JSON.
pub fn encode_domain_event<A: Aggregate>(
    event: &A::DomainEvent,
    ctx: &CommandContext,
    aggregate_type: &str,
    instance_id: &str,
) -> serde_json::Result<ProposedEventData> {
    // Serialize the adjacently-tagged domain event. This produces JSON like:
    //   {"type": "Incremented"}           (unit variant)
    //   {"type": "Added", "data": {...}}  (variant with fields)
    let value = serde_json::to_value(event)?;
    let obj = value
        .as_object()
        .expect("adjacently tagged enum must serialize to a JSON object");

    let event_type = obj["type"]
        .as_str()
        .expect("adjacently tagged enum must have a string 'type' field")
        .to_string();

    // Extract the "data" field; absent for unit variants, so default to null.
    let payload = obj.get("data").cloned().unwrap_or(serde_json::Value::Null);

    let metadata = EventMetadata {
        aggregate_type: aggregate_type.to_string(),
        instance_id: instance_id.to_string(),
        actor: ctx.actor.clone(),
        correlation_id: ctx.correlation_id.clone(),
        source_device: ctx.source_device.clone(),
    };

    Ok(ProposedEventData {
        event_id: Uuid::new_v4(),
        event_type,
        payload,
        metadata,
    })
}

/// Decode a gRPC [`RecordedEvent`](crate::proto::RecordedEvent) into a [`StoredEvent`].
///
/// Parses the `metadata` bytes as JSON, extracts `aggregate_type` and `instance_id`,
/// and constructs a fully-populated `StoredEvent`. Returns `None` if:
///
/// - The metadata bytes are not valid UTF-8 or JSON.
/// - The metadata JSON does not contain both `aggregate_type` and `instance_id`
///   as string fields.
/// - The `event_id` or `stream_id` strings are not valid UUIDs.
///
/// Events from non-eventfold-es clients (missing expected metadata) are
/// silently skipped by returning `None`.
///
/// # Arguments
///
/// * `recorded` - A reference to the proto `RecordedEvent` from the gRPC server.
///
/// # Returns
///
/// `Some(StoredEvent)` if all required fields are present and parseable,
/// `None` otherwise.
pub fn decode_stored_event(recorded: &crate::proto::RecordedEvent) -> Option<StoredEvent> {
    // Parse metadata bytes as UTF-8 then JSON.
    let metadata_str = std::str::from_utf8(&recorded.metadata).ok()?;
    let metadata_value: serde_json::Value = serde_json::from_str(metadata_str).ok()?;

    // Extract required fields from metadata.
    let aggregate_type = metadata_value
        .get("aggregate_type")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let instance_id = metadata_value
        .get("instance_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();

    // Parse UUID strings from the recorded event.
    let event_id = Uuid::parse_str(&recorded.event_id).ok()?;
    let stream_id = Uuid::parse_str(&recorded.stream_id).ok()?;

    // Parse payload bytes as JSON; default to null if empty.
    let payload = if recorded.payload.is_empty() {
        serde_json::Value::Null
    } else {
        let payload_str = std::str::from_utf8(&recorded.payload).ok()?;
        serde_json::from_str(payload_str).ok()?
    };

    Some(StoredEvent {
        event_id,
        stream_id,
        aggregate_type,
        instance_id,
        stream_version: recorded.stream_version,
        global_position: recorded.global_position,
        event_type: recorded.event_type.clone(),
        payload,
        metadata: metadata_value,
        recorded_at: recorded.recorded_at,
    })
}

/// An event as delivered to projections and process managers.
///
/// All fields are pre-extracted from the gRPC `RecordedEvent` and its
/// JSON metadata. Aggregate type and instance ID are recovered from
/// metadata, not from the stream UUID.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    /// Client-assigned event ID.
    pub event_id: uuid::Uuid,
    /// Stream UUID.
    pub stream_id: uuid::Uuid,
    /// Aggregate type extracted from metadata (e.g., "counter").
    pub aggregate_type: String,
    /// Instance ID extracted from metadata (e.g., "c-1").
    pub instance_id: String,
    /// Zero-based version within the stream.
    pub stream_version: u64,
    /// Zero-based position in the global log.
    pub global_position: u64,
    /// Event type tag (e.g., "Incremented").
    pub event_type: String,
    /// Decoded JSON payload (the domain event data).
    pub payload: serde_json::Value,
    /// Decoded JSON metadata (actor, correlation_id, etc.).
    pub metadata: serde_json::Value,
    /// Server-assigned timestamp (Unix epoch milliseconds).
    pub recorded_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_uuid_is_deterministic() {
        let a = stream_uuid("counter", "c-1");
        let b = stream_uuid("counter", "c-1");
        assert_eq!(a, b, "same inputs must produce the same UUID");
    }

    #[test]
    fn stream_uuid_differs_by_instance_id() {
        let a = stream_uuid("counter", "c-1");
        let b = stream_uuid("counter", "c-2");
        assert_ne!(a, b, "different instance IDs must produce different UUIDs");
    }

    #[test]
    fn event_metadata_skips_none_fields_in_serialization() {
        let meta = EventMetadata {
            aggregate_type: "counter".to_string(),
            instance_id: "c-1".to_string(),
            actor: None,
            correlation_id: None,
            source_device: None,
        };
        let json = serde_json::to_string(&meta).expect("serialize should succeed");
        assert!(!json.contains("actor"), "actor should be omitted when None");
        assert!(
            !json.contains("correlation_id"),
            "correlation_id should be omitted when None"
        );
        assert!(
            !json.contains("source_device"),
            "source_device should be omitted when None"
        );
        assert!(json.contains("aggregate_type"));
        assert!(json.contains("instance_id"));
    }

    #[test]
    fn event_metadata_includes_present_fields() {
        let meta = EventMetadata {
            aggregate_type: "counter".to_string(),
            instance_id: "c-1".to_string(),
            actor: Some("user-1".to_string()),
            correlation_id: Some("corr-1".to_string()),
            source_device: Some("dev-1".to_string()),
        };
        let json = serde_json::to_string(&meta).expect("serialize should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse should succeed");
        assert_eq!(parsed["actor"], "user-1");
        assert_eq!(parsed["correlation_id"], "corr-1");
        assert_eq!(parsed["source_device"], "dev-1");
    }

    #[test]
    fn event_metadata_serde_roundtrip() {
        let meta = EventMetadata {
            aggregate_type: "order".to_string(),
            instance_id: "o-42".to_string(),
            actor: Some("admin".to_string()),
            correlation_id: None,
            source_device: Some("laptop".to_string()),
        };
        let json = serde_json::to_string(&meta).expect("serialize should succeed");
        let roundtripped: EventMetadata =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(roundtripped.aggregate_type, "order");
        assert_eq!(roundtripped.instance_id, "o-42");
        assert_eq!(roundtripped.actor.as_deref(), Some("admin"));
        assert_eq!(roundtripped.correlation_id, None);
        assert_eq!(roundtripped.source_device.as_deref(), Some("laptop"));
    }

    #[test]
    fn stream_uuid_differs_by_aggregate_type() {
        let a = stream_uuid("counter", "c-1");
        let b = stream_uuid("order", "c-1");
        assert_ne!(
            a, b,
            "different aggregate types must produce different UUIDs"
        );
    }

    // --- encode_domain_event tests ---

    use crate::aggregate::test_fixtures::{Counter, CounterEvent};
    use crate::command::CommandContext;

    #[test]
    fn encode_incremented_produces_correct_event_type_and_metadata() {
        let ctx = CommandContext::default();
        let result =
            encode_domain_event::<Counter>(&CounterEvent::Incremented, &ctx, "counter", "c-1");
        let proposed = result.expect("encode should succeed");

        assert_eq!(proposed.event_type, "Incremented");
        assert_eq!(proposed.metadata.aggregate_type, "counter");
        assert_eq!(proposed.metadata.instance_id, "c-1");
        // event_id should be a valid UUID v4
        assert_eq!(
            proposed.event_id.get_version(),
            Some(uuid::Version::Random),
            "event_id should be UUID v4"
        );
    }

    #[test]
    fn encode_fieldless_variant_produces_empty_object_or_null_payload() {
        let ctx = CommandContext::default();
        let proposed =
            encode_domain_event::<Counter>(&CounterEvent::Incremented, &ctx, "counter", "c-1")
                .expect("encode should succeed");
        // Fieldless variant: payload should be null or an empty object.
        // The adjacently-tagged format for a unit variant has no "data" key,
        // so we extract null.
        assert!(
            proposed.payload.is_null() || proposed.payload == serde_json::json!({}),
            "fieldless variant payload should be null or {{}}, got: {}",
            proposed.payload
        );
    }

    #[test]
    fn encode_variant_with_data_includes_payload() {
        let ctx = CommandContext::default();
        let proposed = encode_domain_event::<Counter>(
            &CounterEvent::Added { amount: 42 },
            &ctx,
            "counter",
            "c-1",
        )
        .expect("encode should succeed");

        assert_eq!(proposed.event_type, "Added");
        assert_eq!(proposed.payload["amount"], 42);
    }

    #[test]
    fn encode_populates_all_optional_metadata_fields() {
        let ctx = CommandContext::default()
            .with_actor("u1")
            .with_correlation_id("c1")
            .with_source_device("d1");
        let proposed =
            encode_domain_event::<Counter>(&CounterEvent::Incremented, &ctx, "counter", "c-1")
                .expect("encode should succeed");

        assert_eq!(proposed.metadata.actor.as_deref(), Some("u1"));
        assert_eq!(proposed.metadata.correlation_id.as_deref(), Some("c1"));
        assert_eq!(proposed.metadata.source_device.as_deref(), Some("d1"));
    }

    #[test]
    fn encode_omits_none_optional_metadata_fields() {
        let ctx = CommandContext::default();
        let proposed =
            encode_domain_event::<Counter>(&CounterEvent::Incremented, &ctx, "counter", "c-1")
                .expect("encode should succeed");

        assert_eq!(proposed.metadata.actor, None);
        assert_eq!(proposed.metadata.correlation_id, None);
        assert_eq!(proposed.metadata.source_device, None);
    }

    // --- decode_stored_event tests ---

    use crate::proto::RecordedEvent;

    fn make_recorded_event_with_metadata(metadata_json: &str) -> RecordedEvent {
        let event_id = Uuid::new_v4().to_string();
        let stream_id = stream_uuid("counter", "c-1").to_string();
        RecordedEvent {
            event_id,
            stream_id,
            stream_version: 3,
            global_position: 42,
            event_type: "Incremented".to_string(),
            metadata: metadata_json.as_bytes().to_vec(),
            payload: b"{}".to_vec(),
            recorded_at: 1_700_000_000_000,
        }
    }

    #[test]
    fn decode_well_formed_recorded_event_returns_some() {
        let meta_json = r#"{"aggregate_type":"counter","instance_id":"c-1","actor":"u1"}"#;
        let recorded = make_recorded_event_with_metadata(meta_json);
        let stored = decode_stored_event(&recorded);
        assert!(stored.is_some(), "well-formed metadata should decode");

        let stored = stored.expect("already checked");
        assert_eq!(stored.aggregate_type, "counter");
        assert_eq!(stored.instance_id, "c-1");
        assert_eq!(stored.stream_version, 3);
        assert_eq!(stored.global_position, 42);
        assert_eq!(stored.event_type, "Incremented");
        assert_eq!(stored.recorded_at, 1_700_000_000_000);
        // stream_id should be parsed from the recorded event
        assert_eq!(
            stored.stream_id,
            Uuid::parse_str(&stream_uuid("counter", "c-1").to_string()).expect("valid uuid")
        );
    }

    #[test]
    fn decode_empty_json_object_returns_none() {
        let recorded = make_recorded_event_with_metadata("{}");
        let stored = decode_stored_event(&recorded);
        assert!(
            stored.is_none(),
            "metadata missing aggregate_type should return None"
        );
    }

    #[test]
    fn decode_invalid_json_returns_none() {
        let event_id = Uuid::new_v4().to_string();
        let stream_id = Uuid::new_v4().to_string();
        let recorded = RecordedEvent {
            event_id,
            stream_id,
            stream_version: 0,
            global_position: 0,
            event_type: "Test".to_string(),
            metadata: vec![0xFF, 0xFE], // invalid UTF-8/JSON
            payload: vec![],
            recorded_at: 0,
        };
        let stored = decode_stored_event(&recorded);
        assert!(
            stored.is_none(),
            "invalid UTF-8/JSON metadata should return None"
        );
    }

    #[test]
    fn decode_missing_instance_id_returns_none() {
        let meta_json = r#"{"aggregate_type":"counter"}"#;
        let recorded = make_recorded_event_with_metadata(meta_json);
        let stored = decode_stored_event(&recorded);
        assert!(
            stored.is_none(),
            "metadata missing instance_id should return None"
        );
    }
}
