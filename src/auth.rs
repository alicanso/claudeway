use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::Arc;
use crate::error::{ApiError, AppError};

/// Newtype struct representing an authenticated key ID extracted from Bearer token
#[derive(Debug, Clone)]
pub struct KeyId(pub String);

/// Auth middleware that validates Bearer tokens and extracts key IDs
///
/// Extracts the Authorization header, validates it starts with "Bearer ",
/// looks up the token in the api_keys HashMap, and inserts the KeyId
/// into request extensions if valid.
pub async fn auth_middleware(
    mut request: Request,
    next: Next,
    api_keys: Arc<HashMap<String, String>>,
) -> Result<Response, AppError> {
    // Extract Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..]; // Skip "Bearer " prefix

            // Look up token in api_keys HashMap
            if let Some(key_id) = api_keys.get(token) {
                // Insert KeyId into request extensions
                request.extensions_mut().insert(KeyId(key_id.clone()));
                Ok(next.run(request).await)
            } else {
                // Token not found in keys
                Err(ApiError::unauthorized())
            }
        }
        _ => {
            // Missing or invalid Authorization header
            Err(ApiError::unauthorized())
        }
    }
}
