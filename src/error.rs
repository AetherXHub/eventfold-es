//! Crate-level error types for command execution and state retrieval.

/// Error returned when executing a command against an aggregate fails.
///
/// Generic over `E`, the domain-specific error type that the aggregate's
/// command handler may produce (e.g., "insufficient funds").
///
/// # Type Parameters
///
/// * `E` - Domain error type, must implement `Error + Send + Sync + 'static`
#[derive(Debug, thiserror::Error)]
pub enum ExecuteError<E: std::error::Error + Send + Sync + 'static> {
    /// Command rejected by aggregate logic.
    ///
    /// Wraps the domain-specific error returned from the aggregate's
    /// command handler, forwarding its `Display` and `Error` impls.
    #[error(transparent)]
    Domain(E),

    /// Optimistic concurrency retries exhausted.
    ///
    /// The command was retried the maximum number of times but each
    /// attempt encountered a stream version mismatch with a concurrent writer.
    #[error("wrong expected version: retries exhausted")]
    WrongExpectedVersion,

    /// Disk I/O failure.
    ///
    /// An underlying filesystem or storage-layer I/O error occurred
    /// while loading or persisting events.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// gRPC transport or server error.
    ///
    /// The underlying tonic gRPC call returned a non-OK status, indicating
    /// a network failure, server error, or protocol-level rejection.
    #[error("gRPC transport error: {0}")]
    Transport(tonic::Status),

    /// Actor thread exited unexpectedly.
    ///
    /// The background actor that owns this aggregate has shut down,
    /// so no further commands can be processed.
    #[error("aggregate actor is no longer running")]
    ActorGone,
}

/// Error returned when reading the current state of an aggregate fails.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Disk I/O failure.
    ///
    /// An underlying filesystem or storage-layer I/O error occurred
    /// while loading events to rebuild the aggregate state.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// gRPC transport or server error.
    ///
    /// The underlying tonic gRPC call returned a non-OK status, indicating
    /// a network failure, server error, or protocol-level rejection.
    #[error("gRPC transport error: {0}")]
    Transport(tonic::Status),

    /// Actor thread exited unexpectedly.
    ///
    /// The background actor that owns this aggregate has shut down,
    /// so its state can no longer be queried.
    #[error("aggregate actor is no longer running")]
    ActorGone,
}

/// Errors that can occur when dispatching a command.
///
/// Produced by the process manager dispatch layer when a command envelope
/// cannot be routed to or executed by the target aggregate.
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    /// No dispatcher registered for the target aggregate type.
    #[error("unknown aggregate type: {0}")]
    UnknownAggregateType(String),

    /// No route registered for the command type.
    ///
    /// Returned when the command's target aggregate type has not been
    /// registered via [`AggregateStoreBuilder::aggregate_type`](crate::AggregateStoreBuilder::aggregate_type).
    #[error("no route registered for command type")]
    UnknownCommand,

    /// The command JSON could not be deserialized into the target
    /// aggregate's command type.
    #[error("command deserialization failed: {0}")]
    Deserialization(serde_json::Error),

    /// The target aggregate's command handler rejected the command or
    /// an I/O error occurred during execution.
    #[error("command execution failed: {0}")]
    Execution(Box<dyn std::error::Error + Send + Sync>),

    /// An I/O error occurred during dispatch (e.g. directory creation).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal domain error for testing `ExecuteError<E>`.
    #[derive(Debug, thiserror::Error)]
    #[error("test domain error")]
    struct TestDomainError;

    #[test]
    fn execute_error_domain_displays_inner() {
        let err: ExecuteError<TestDomainError> = ExecuteError::Domain(TestDomainError);
        assert_eq!(err.to_string(), "test domain error");
    }

    #[test]
    fn execute_error_wrong_expected_version_display_message() {
        let err: ExecuteError<TestDomainError> = ExecuteError::WrongExpectedVersion;
        assert_eq!(err.to_string(), "wrong expected version: retries exhausted");
    }

    #[test]
    fn execute_error_io_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: ExecuteError<TestDomainError> = ExecuteError::from(io_err);
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn execute_error_actor_gone_display() {
        let err: ExecuteError<TestDomainError> = ExecuteError::ActorGone;
        assert_eq!(err.to_string(), "aggregate actor is no longer running");
    }

    #[test]
    fn state_error_io_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: StateError = StateError::from(io_err);
        assert!(err.to_string().contains("access denied"));
    }

    #[test]
    fn state_error_actor_gone_display() {
        let err = StateError::ActorGone;
        assert_eq!(err.to_string(), "aggregate actor is no longer running");
    }

    #[test]
    fn execute_error_wrong_expected_version_display() {
        let err: ExecuteError<TestDomainError> = ExecuteError::WrongExpectedVersion;
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }

    #[test]
    fn execute_error_transport_display() {
        let status = tonic::Status::internal("boom");
        let err: ExecuteError<TestDomainError> = ExecuteError::Transport(status);
        let msg = err.to_string();
        assert!(
            msg.contains("boom") || msg.contains("gRPC"),
            "expected 'boom' or 'gRPC' in: {msg}"
        );
    }

    #[test]
    fn state_error_transport_display() {
        let status = tonic::Status::unavailable("server down");
        let err = StateError::Transport(status);
        let msg = err.to_string();
        assert!(
            msg.contains("server down") || msg.contains("gRPC"),
            "expected 'server down' or 'gRPC' in: {msg}"
        );
    }

    // Verify `Send + Sync` bounds are satisfied so errors can cross thread
    // boundaries, which is required for use with `tokio` channels.
    const _: () = {
        #[allow(dead_code)]
        fn assert_send_sync<T: Send + Sync>() {}

        #[allow(dead_code)]
        fn check() {
            assert_send_sync::<ExecuteError<TestDomainError>>();
            assert_send_sync::<StateError>();
        }
    };
}
