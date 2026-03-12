use axum::Json;
use std::sync::Arc;
use std::time::Instant;

use crate::models::HealthResponse;

#[utoipa::path(
    get,
    path = "/health",
    tag = "System",
    summary = "Health check",
    description = "Returns server status, version, and uptime. No authentication required.",
    responses(
        (status = 200, description = "Server is healthy", body = HealthResponse)
    )
)]
pub async fn health(start_time: Arc<Instant>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: start_time.elapsed().as_secs(),
    })
}
