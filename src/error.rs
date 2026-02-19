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
    /// attempt encountered a version conflict with a concurrent writer.
    #[error("optimistic concurrency conflict: retries exhausted")]
    Conflict,

    /// Disk I/O failure.
    ///
    /// An underlying filesystem or storage-layer I/O error occurred
    /// while loading or persisting events.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

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

    /// Actor thread exited unexpectedly.
    ///
    /// The background actor that owns this aggregate has shut down,
    /// so its state can no longer be queried.
    #[error("aggregate actor is no longer running")]
    ActorGone,
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
    fn execute_error_conflict_display() {
        let err: ExecuteError<TestDomainError> = ExecuteError::Conflict;
        assert_eq!(
            err.to_string(),
            "optimistic concurrency conflict: retries exhausted"
        );
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
