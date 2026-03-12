use axum::extract::{Extension, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::admin_auth::AdminSessionStore;
use crate::admin_models::*;
use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::logging::KeyLogger;
use crate::session::SessionStore;

/// POST /admin/login
pub async fn login(
    Extension(config): Extension<Arc<Config>>,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Json(req): Json<AdminLoginRequest>,
) -> Result<Response, AppError> {
    let key_id = config.api_keys.get(&req.key);
    match key_id {
        Some(id) if *id == config.admin_key_id => {
            let token = admin_sessions.create_session();
            let cookie_value = format!(
                "admin_token={}; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=3600",
                token
            );
            let body = serde_json::to_string(&AdminLoginResponse { success: true }).unwrap();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Set-Cookie", cookie_value)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap())
        }
        _ => Err(ApiError::unauthorized()),
    }
}

/// Extract and validate admin token from cookie
fn validate_admin_cookie(headers: &HeaderMap, store: &AdminSessionStore) -> Result<(), AppError> {
    let cookie_header = headers.get("Cookie").and_then(|v| v.to_str().ok()).unwrap_or("");
    let token = cookie_header
        .split(';')
        .filter_map(|c| { let c = c.trim(); c.strip_prefix("admin_token=") })
        .next();
    match token {
        Some(t) if store.validate(t) => Ok(()),
        _ => Err(ApiError::unauthorized()),
    }
}

/// GET /admin/overview
pub async fn overview(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(start_time): Extension<Arc<Instant>>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
) -> Result<Json<OverviewResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let sessions = store.list_all();
    let total_cost: f64 = sessions.iter().map(|s| s.cost_usd).sum();
    let models_breakdown = crate::admin_stats::get_models_breakdown(logger.log_dir());
    Ok(Json(OverviewResponse {
        uptime_secs: start_time.elapsed().as_secs(),
        total_requests: request_counter.load(Ordering::Relaxed),
        active_sessions: sessions.len() as u64,
        total_cost_usd: total_cost,
        models_breakdown,
    }))
}

/// GET /admin/sessions
pub async fn list_sessions(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsListResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).min(100);
    let mut sessions: Vec<_> = store.list_all();
    if let Some(ref model) = query.model {
        sessions.retain(|s| s.model.as_deref() == Some(model.as_str()));
    }
    let total = sessions.len() as u64;
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let start = ((page - 1) * limit) as usize;
    let page_sessions: Vec<SessionSummary> = sessions.into_iter().skip(start).take(limit as usize)
        .map(|s| SessionSummary {
            session_id: s.session_id.to_string(), created_at: s.created_at,
            last_used: s.last_used, model: s.model, task_count: s.task_count,
            cost_usd: s.cost_usd, key_id: s.key_id,
        }).collect();
    Ok(Json(SessionsListResponse { sessions: page_sessions, total, page, limit }))
}

/// GET /admin/sessions/:id
pub async fn get_session_detail(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetailResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let session_id = uuid::Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid session ID"))?;
    let session = store.get(&session_id).ok_or_else(|| ApiError::not_found("Session not found"))?;
    Ok(Json(SessionDetailResponse {
        session_id: session.session_id.to_string(), created_at: session.created_at,
        last_used: session.last_used, model: session.model, task_count: session.task_count,
        cost_usd: session.cost_usd, key_id: session.key_id, tokens: session.tokens,
        workdir: session.workdir.to_string_lossy().to_string(),
    }))
}

/// GET /admin/logs
pub async fn get_logs(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let log_dir = logger.log_dir();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let key_dirs: Vec<_> = if let Some(ref key_id) = query.key_id {
        vec![log_dir.join(key_id)]
    } else {
        std::fs::read_dir(log_dir).map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default()
    };
    let month_str = match query.date.as_deref() {
        Some(d) if d.len() >= 7 => d[..7].to_string(),
        _ => Utc::now().format("%Y-%m").to_string(),
    };
    for dir in key_dirs {
        if !dir.is_dir() { continue; }
        let log_file = dir.join(format!("{}.log", month_str));
        if !log_file.exists() { continue; }
        if let Ok(content) = std::fs::read_to_string(&log_file) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(ref after) = query.after {
                        if let Some(ts) = value.get("timestamp").and_then(|v| v.as_str()) {
                            if ts <= after.as_str() { continue; }
                        }
                    }
                    entries.push(value);
                }
            }
        }
    }
    entries.sort_by(|a, b| {
        let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        ts_b.cmp(ts_a)
    });
    let limit = query.limit.unwrap_or(100) as usize;
    let total = entries.len() as u64;
    entries.truncate(limit);
    Ok(Json(LogsResponse { entries, total }))
}

/// GET /admin/keys
pub async fn get_keys(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
) -> Result<Json<KeysResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let log_dir = logger.log_dir();
    let key_ids: Vec<String> = config.key_ids().into_iter().cloned().collect();
    let keys = crate::admin_stats::get_keys_stats(log_dir, &key_ids);
    Ok(Json(KeysResponse { keys }))
}

/// GET /admin/costs
pub async fn get_costs(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Query(query): Query<CostsQuery>,
) -> Result<Json<CostsResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;
    let log_dir = logger.log_dir();
    let group_by = query.group_by.as_deref().unwrap_or("daily");
    let data = crate::admin_stats::aggregate_costs(log_dir, group_by);
    Ok(Json(CostsResponse { data }))
}
