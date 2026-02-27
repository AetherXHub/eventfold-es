//! Thin, typed wrapper around the tonic-generated `EventStoreClient`.
//!
//! Provides ergonomic async methods ([`EsClient::append`], [`EsClient::read_stream`],
//! [`EsClient::subscribe_all_from`]) that accept and return Rust-native types so
//! that the actor and projection modules never import tonic internals directly.

use crate::event::ProposedEventData;
use crate::proto;
use crate::proto::event_store_client::EventStoreClient;
use tonic::transport::Channel;
use uuid::Uuid;

/// Local enum representing the expected stream version for optimistic concurrency.
///
/// Converted to the proto [`ExpectedVersion`](proto::ExpectedVersion) before
/// being sent over the wire. This insulates callers from the proto oneof encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersionArg {
    /// Accept any current stream version (no concurrency check).
    Any,
    /// The stream must not exist yet (first write).
    NoStream,
    /// The stream must be at exactly this version.
    Exact(u64),
}

impl ExpectedVersionArg {
    /// Convert to the proto [`ExpectedVersion`](proto::ExpectedVersion) message.
    ///
    /// # Returns
    ///
    /// A fully-populated proto `ExpectedVersion` with the appropriate `kind` variant.
    pub fn to_proto(self) -> proto::ExpectedVersion {
        let kind = match self {
            Self::Any => proto::expected_version::Kind::Any(proto::Empty {}),
            Self::NoStream => proto::expected_version::Kind::NoStream(proto::Empty {}),
            Self::Exact(v) => proto::expected_version::Kind::Exact(v),
        };
        proto::ExpectedVersion { kind: Some(kind) }
    }
}

/// Convert a [`ProposedEventData`] into the proto [`ProposedEvent`](proto::ProposedEvent).
///
/// Serializes the JSON payload and metadata as UTF-8 bytes for the gRPC wire format.
/// This function is extracted from [`EsClient::append`] so that conversion logic
/// can be unit-tested without a live gRPC connection.
///
/// # Arguments
///
/// * `data` - The Rust-native proposed event to convert.
///
/// # Returns
///
/// A proto `ProposedEvent` with `event_id`, `event_type`, `payload`, and `metadata`
/// fields populated from the input.
pub fn to_proto_event(data: &ProposedEventData) -> proto::ProposedEvent {
    // Serialize payload and metadata to JSON bytes. These serializations
    // are infallible for types that are already valid serde_json::Value /
    // EventMetadata, so Vec::new() fallback is defensive.
    let payload = serde_json::to_vec(&data.payload).unwrap_or_default();
    let metadata = serde_json::to_vec(&data.metadata).unwrap_or_default();

    proto::ProposedEvent {
        event_id: data.event_id.to_string(),
        event_type: data.event_type.clone(),
        payload,
        metadata,
    }
}

/// Typed gRPC client for the `eventfold-db` event store.
///
/// Wraps the tonic-generated [`EventStoreClient<Channel>`] and exposes
/// ergonomic async methods that accept Rust-native types. Clone is cheap
/// because tonic channels use internal reference counting.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use eventfold_es::EsClient;
///
/// let mut client = EsClient::connect("http://127.0.0.1:2113").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct EsClient {
    inner: EventStoreClient<Channel>,
}

impl EsClient {
    /// Connect to an `eventfold-db` gRPC server at the given endpoint.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The URI of the gRPC server (e.g., `"http://127.0.0.1:2113"`).
    ///
    /// # Returns
    ///
    /// An `EsClient` ready to issue RPCs.
    ///
    /// # Errors
    ///
    /// Returns [`tonic::transport::Error`] if the channel cannot be established.
    pub async fn connect(endpoint: &str) -> Result<Self, tonic::transport::Error> {
        let inner = EventStoreClient::connect(endpoint.to_string()).await?;
        Ok(Self { inner })
    }

    /// Construct an `EsClient` from a pre-built [`EventStoreClient`].
    ///
    /// Used in tests to create clients with lazy or mock channels.
    #[cfg(test)]
    pub(crate) fn from_inner(inner: EventStoreClient<Channel>) -> Self {
        Self { inner }
    }

    /// Append events to a stream with optimistic concurrency control.
    ///
    /// Converts each [`ProposedEventData`] into a proto `ProposedEvent` (serializing
    /// payload and metadata as JSON bytes) and sends an `Append` RPC.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The UUID of the target stream.
    /// * `expected` - The expected current stream version for optimistic concurrency.
    /// * `events` - The domain events to append, already encoded as [`ProposedEventData`].
    ///
    /// # Returns
    ///
    /// The server's [`AppendResponse`](proto::AppendResponse) containing the
    /// assigned stream versions and global positions.
    ///
    /// # Errors
    ///
    /// Returns [`tonic::Status`] on transport errors, version conflicts
    /// (`FAILED_PRECONDITION`), or server-side failures.
    pub async fn append(
        &mut self,
        stream_id: Uuid,
        expected: ExpectedVersionArg,
        events: Vec<ProposedEventData>,
    ) -> Result<proto::AppendResponse, tonic::Status> {
        let proto_events: Vec<proto::ProposedEvent> = events.iter().map(to_proto_event).collect();

        let request = proto::AppendRequest {
            stream_id: stream_id.to_string(),
            expected_version: Some(expected.to_proto()),
            events: proto_events,
        };

        let response = self.inner.append(request).await?;
        Ok(response.into_inner())
    }

    /// Read events from a single stream starting at a given version.
    ///
    /// # Arguments
    ///
    /// * `stream_id` - The UUID of the stream to read.
    /// * `from_version` - The zero-based stream version to start reading from.
    /// * `max_count` - Maximum number of events to return.
    ///
    /// # Returns
    ///
    /// A `Vec` of proto [`RecordedEvent`](proto::RecordedEvent) messages in
    /// stream-version order.
    ///
    /// # Errors
    ///
    /// Returns [`tonic::Status`] on transport or server-side errors.
    pub async fn read_stream(
        &mut self,
        stream_id: Uuid,
        from_version: u64,
        max_count: u64,
    ) -> Result<Vec<proto::RecordedEvent>, tonic::Status> {
        let request = proto::ReadStreamRequest {
            stream_id: stream_id.to_string(),
            from_version,
            max_count,
        };

        match self.inner.read_stream(request).await {
            Ok(response) => Ok(response.into_inner().events),
            // A stream that has never been written to returns NotFound.
            // Treat this as an empty event list rather than an error,
            // since actors need to catch up on streams that may not exist yet.
            Err(status) if status.code() == tonic::Code::NotFound => Ok(Vec::new()),
            Err(status) => Err(status),
        }
    }

    /// Subscribe to all events in the global log from a given position.
    ///
    /// Returns a streaming response that yields [`SubscribeResponse`](proto::SubscribeResponse)
    /// messages (either a `RecordedEvent` or a `CaughtUp` sentinel). The stream
    /// remains open until the server closes it or the client drops the stream.
    ///
    /// # Arguments
    ///
    /// * `from_position` - The zero-based global position to start subscribing from.
    ///
    /// # Returns
    ///
    /// A [`tonic::Streaming`] that implements `Stream<Item = Result<SubscribeResponse, Status>>`.
    ///
    /// # Errors
    ///
    /// Returns [`tonic::Status`] if the initial RPC handshake fails.
    pub async fn subscribe_all_from(
        &mut self,
        from_position: u64,
    ) -> Result<tonic::Streaming<proto::SubscribeResponse>, tonic::Status> {
        let request = proto::SubscribeAllRequest { from_position };
        let response = self.inner.subscribe_all(request).await?;
        Ok(response.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventMetadata, ProposedEventData};
    use uuid::Uuid;

    // --- to_proto_event conversion tests ---

    #[test]
    fn to_proto_event_roundtrip_payload_and_metadata() {
        let proposed = ProposedEventData {
            event_id: Uuid::new_v4(),
            event_type: "Incremented".to_string(),
            payload: serde_json::json!({"amount": 42}),
            metadata: EventMetadata {
                aggregate_type: "counter".to_string(),
                instance_id: "c-1".to_string(),
                actor: Some("user-1".to_string()),
                correlation_id: None,
                source_device: None,
            },
        };

        let proto = to_proto_event(&proposed);

        // event_id and event_type round-trip exactly.
        assert_eq!(proto.event_id, proposed.event_id.to_string());
        assert_eq!(proto.event_type, "Incremented");

        // Payload bytes decode back to the original JSON value.
        let payload_decoded: serde_json::Value =
            serde_json::from_slice(&proto.payload).expect("payload should be valid JSON");
        assert_eq!(payload_decoded, serde_json::json!({"amount": 42}));

        // Metadata bytes decode back to the original EventMetadata.
        let meta_decoded: EventMetadata =
            serde_json::from_slice(&proto.metadata).expect("metadata should be valid JSON");
        assert_eq!(meta_decoded.aggregate_type, "counter");
        assert_eq!(meta_decoded.instance_id, "c-1");
        assert_eq!(meta_decoded.actor.as_deref(), Some("user-1"));
        assert_eq!(meta_decoded.correlation_id, None);
    }

    #[test]
    fn to_proto_event_null_payload_serializes_as_null() {
        let proposed = ProposedEventData {
            event_id: Uuid::new_v4(),
            event_type: "Incremented".to_string(),
            payload: serde_json::Value::Null,
            metadata: EventMetadata {
                aggregate_type: "counter".to_string(),
                instance_id: "c-1".to_string(),
                actor: None,
                correlation_id: None,
                source_device: None,
            },
        };

        let proto = to_proto_event(&proposed);
        let payload_decoded: serde_json::Value =
            serde_json::from_slice(&proto.payload).expect("payload should be valid JSON");
        assert_eq!(payload_decoded, serde_json::Value::Null);
    }

    // --- ExpectedVersionArg conversion tests ---

    #[test]
    fn expected_version_any_converts_to_proto() {
        let proto = ExpectedVersionArg::Any.to_proto();
        assert!(
            matches!(
                proto.kind,
                Some(crate::proto::expected_version::Kind::Any(_))
            ),
            "Any should map to proto Any variant"
        );
    }

    #[test]
    fn expected_version_no_stream_converts_to_proto() {
        let proto = ExpectedVersionArg::NoStream.to_proto();
        assert!(
            matches!(
                proto.kind,
                Some(crate::proto::expected_version::Kind::NoStream(_))
            ),
            "NoStream should map to proto NoStream variant"
        );
    }

    #[test]
    fn expected_version_exact_converts_to_proto() {
        let proto = ExpectedVersionArg::Exact(5).to_proto();
        assert!(
            matches!(
                proto.kind,
                Some(crate::proto::expected_version::Kind::Exact(5))
            ),
            "Exact(5) should map to proto Exact(5) variant"
        );
    }
}
