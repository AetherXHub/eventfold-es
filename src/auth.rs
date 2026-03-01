//! gRPC interceptor that injects a Bearer token into outgoing requests.

use std::sync::{Arc, RwLock};

/// gRPC interceptor that injects a Bearer token from a shared, refreshable
/// string.
///
/// The token is read from the [`RwLock`] on every intercepted request using
/// a synchronous `read()` lock because tonic interceptors are called
/// synchronously. If the token string is empty, no `authorization` header
/// is added.
///
/// # Panics
///
/// Panics if the inner [`RwLock`] is poisoned (i.e. a writer panicked while
/// holding the write lock). This is treated as an invariant violation.
#[derive(Clone)]
pub(crate) struct BearerInterceptor {
    /// Shared, refreshable token string. An empty string means "no auth".
    pub(crate) token: Arc<RwLock<String>>,
}

impl tonic::service::Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let token = self.token.read().expect("token RwLock poisoned");
        if !token.is_empty() {
            let value = format!("Bearer {token}")
                .parse::<tonic::metadata::MetadataValue<_>>()
                .map_err(|_| tonic::Status::internal("invalid token characters"))?;
            req.metadata_mut().insert("authorization", value);
        }
        Ok(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::service::Interceptor;

    #[test]
    fn non_empty_token_inserts_bearer_header() {
        let mut interceptor = BearerInterceptor {
            token: Arc::new(RwLock::new("abc".to_string())),
        };
        let request = tonic::Request::new(());
        let result = interceptor.call(request).expect("call should succeed");
        let value = result
            .metadata()
            .get("authorization")
            .expect("authorization header should be present");
        assert_eq!(value, "Bearer abc");
    }

    #[test]
    fn empty_token_omits_authorization_header() {
        let mut interceptor = BearerInterceptor {
            token: Arc::new(RwLock::new(String::new())),
        };
        let request = tonic::Request::new(());
        let result = interceptor.call(request).expect("call should succeed");
        assert!(
            result.metadata().get("authorization").is_none(),
            "authorization header should not be present for empty token"
        );
    }

    #[test]
    fn token_mutation_visible_on_next_call() {
        let mut interceptor = BearerInterceptor {
            token: Arc::new(RwLock::new("abc".to_string())),
        };

        // First call: token is "abc".
        let result = interceptor
            .call(tonic::Request::new(()))
            .expect("call should succeed");
        let value = result
            .metadata()
            .get("authorization")
            .expect("authorization header should be present");
        assert_eq!(value, "Bearer abc");

        // Mutate the token in-place.
        *interceptor.token.write().unwrap() = "xyz".to_string();

        // Second call: token should now be "xyz".
        let result = interceptor
            .call(tonic::Request::new(()))
            .expect("call should succeed");
        let value = result
            .metadata()
            .get("authorization")
            .expect("authorization header should be present");
        assert_eq!(value, "Bearer xyz");
    }
}
