use axum::extract::{Extension, Path};
use axum::Json;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::KeyId;
use crate::claude;
use crate::config::Config;
use crate::error::ApiError;
use crate::logging::ClaudeInvocationLog;
use crate::models::{
    DeleteSessionResponse, SessionContinueRequest, SessionInfoResponse, SessionStartRequest,
    SessionStartResponse, TaskResponse, TokenUsage,
};
use crate::plugin::{GatewayEvent, PluginContext};
use crate::session::{SessionMeta, SessionStore};

#[utoipa::path(
    post,
    path = "/session/start",
    tag = "Sessions",
    summary = "Start a new session",
    description = "Create a persistent Claude session with an isolated working directory.",
    security(("bearer" = [])),
    request_body = SessionStartRequest,
    responses(
        (status = 200, description = "Session created", body = SessionStartResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError),
        (status = 500, description = "Internal error", body = crate::error::ApiError)
    )
)]
pub async fn start_session(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(plugin_ctx): Extension<PluginContext>,
    Json(req): Json<SessionStartRequest>,
) -> Result<Json<SessionStartResponse>, crate::error::AppError> {
    let session_id = Uuid::new_v4();
    let created_at = Utc::now();

    // Determine workdir and auto_workdir flag
    let (workdir, auto_workdir) = if let Some(provided_workdir) = &req.workdir {
        (PathBuf::from(provided_workdir), false)
    } else {
        let auto_workdir_path = PathBuf::from(&config.claude_workdir).join(session_id.to_string());
        (auto_workdir_path, true)
    };

    // Create workdir
    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create workdir: {}", e)))?;

    // Clone model before moving into SessionMeta
    let model_name = req.model.clone().unwrap_or_else(|| "default".to_string());

    // Create SessionMeta
    let meta = SessionMeta {
        session_id,
        claude_session_id: None,
        created_at,
        last_used: created_at,
        model: req.model,
        system_prompt: req.system_prompt,
        workdir: workdir.clone(),
        auto_workdir,
        task_count: 0,
        tokens: TokenUsage::default(),
        cost_usd: 0.0,
        key_id: key_id.0.clone(),
    };

    // Insert into store
    store.insert(meta);

    // Emit session started event
    plugin_ctx.emit(GatewayEvent::SessionStarted {
        session_id: session_id.to_string(),
        model: model_name,
        key_id: key_id.0.clone(),
    });

    Ok(Json(SessionStartResponse {
        session_id: session_id.to_string(),
        workdir: workdir.to_string_lossy().to_string(),
        created_at,
    }))
}

#[utoipa::path(
    post,
    path = "/session/{id}",
    tag = "Sessions",
    summary = "Continue a session",
    description = "Send a new prompt to an existing session. Uses --resume for conversation continuity. Concurrent requests are serialized per-session.",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Session UUID")),
    request_body = SessionContinueRequest,
    responses(
        (status = 200, description = "Task completed", body = TaskResponse),
        (status = 400, description = "Bad request", body = crate::error::ApiError),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError),
        (status = 404, description = "Session not found", body = crate::error::ApiError),
        (status = 408, description = "Timeout", body = crate::error::ApiError)
    )
)]
#[allow(clippy::too_many_arguments)]
pub async fn continue_session(
    Extension(key_id): Extension<KeyId>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(logger): Extension<Arc<crate::logging::KeyLogger>>,
    Extension(plugin_ctx): Extension<PluginContext>,
    Path(id): Path<String>,
    Json(req): Json<SessionContinueRequest>,
) -> Result<Json<TaskResponse>, crate::error::AppError> {
    request_counter.fetch_add(1, Ordering::Relaxed);

    // Parse UUID from path
    let session_id = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    // Validate prompt not empty
    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("Prompt cannot be empty"));
    }

    // Get session from store
    let session = store
        .get(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    // Acquire per-session lock
    let lock = store
        .get_lock(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    let _guard = lock.lock().await;

    // Determine timeout
    let timeout_secs = req.timeout_secs.unwrap_or(600);

    // Call claude run function
    let claude_result = if let Some(ref claude_session_id) = session.claude_session_id {
        // Resume existing session
        claude::run_resume(
            &config,
            &req.prompt,
            claude_session_id,
            &session.workdir,
            timeout_secs,
        )
        .await?
    } else {
        // First message - run as new task
        claude::run_task(
            &config,
            &req.prompt,
            session.model.as_deref(),
            session.system_prompt.as_deref(),
            &session.workdir,
            timeout_secs,
        )
        .await?
    };

    // Update session metadata
    store.update(&session_id, |meta| {
        // Set claude_session_id if this is the first invocation
        if meta.claude_session_id.is_none() {
            meta.claude_session_id = claude_result.claude_session_id.clone();
        }
        // Update last_used
        meta.last_used = Utc::now();
        // Increment task_count
        meta.task_count += 1;
        // Accumulate tokens
        if let Some(tokens) = &claude_result.tokens {
            meta.tokens.accumulate(tokens);
        }
        // Add cost
        if let Some(cost) = claude_result.cost_usd {
            meta.cost_usd += cost;
        }
    });

    // Save values for event emission before moving into log entry
    let event_key_id = key_id.0.clone();
    let event_model = session.model.clone().unwrap_or_else(|| "unknown".to_string());

    // Log the invocation
    let log_entry = ClaudeInvocationLog {
        timestamp: Utc::now().to_rfc3339(),
        level: "INFO",
        key_id: key_id.0,
        session_id: session_id.to_string(),
        model: session.model,
        exit_code: claude_result.exit_code,
        duration_ms: claude_result.duration_ms,
        success: claude_result.success,
        tokens: claude_result.tokens.clone(),
        cost_usd: claude_result.cost_usd,
        message: format!("Claude invocation for session {}", session_id),
    };
    logger.log_claude_invocation(&log_entry);

    // Emit events
    if let Some(ref tokens) = claude_result.tokens {
        plugin_ctx.emit(GatewayEvent::SessionCompleted {
            session_id: session_id.to_string(),
            token_usage: tokens.clone(),
        });
    }
    if let Some(cost) = claude_result.cost_usd {
        plugin_ctx.emit(GatewayEvent::CostRecorded {
            key_id: event_key_id,
            model: event_model,
            cost,
        });
    }

    Ok(Json(TaskResponse {
        session_id: session_id.to_string(),
        result: claude_result.result,
        success: claude_result.success,
        duration_ms: claude_result.duration_ms,
        tokens: claude_result.tokens,
        cost_usd: claude_result.cost_usd,
        error: if !claude_result.success {
            Some("Claude invocation failed".to_string())
        } else {
            None
        },
    }))
}

#[utoipa::path(
    get,
    path = "/session/{id}",
    tag = "Sessions",
    summary = "Get session info",
    description = "Returns session metadata including cumulative token usage and cost.",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Session UUID")),
    responses(
        (status = 200, description = "Session info", body = SessionInfoResponse),
        (status = 400, description = "Bad request", body = crate::error::ApiError),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError),
        (status = 404, description = "Session not found", body = crate::error::ApiError)
    )
)]
pub async fn get_session(
    Extension(store): Extension<Arc<SessionStore>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfoResponse>, crate::error::AppError> {
    // Parse UUID from path
    let session_id = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    // Get session from store
    let session = store
        .get(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    Ok(Json(SessionInfoResponse {
        session_id: session_id.to_string(),
        created_at: session.created_at,
        last_used: session.last_used,
        model: session.model,
        task_count: session.task_count as u64,
        workdir: session.workdir.to_string_lossy().to_string(),
        tokens: session.tokens,
        cost_usd: session.cost_usd,
    }))
}

#[utoipa::path(
    delete,
    path = "/session/{id}",
    tag = "Sessions",
    summary = "Delete a session",
    description = "Remove session from store and clean up auto-allocated working directory.",
    security(("bearer" = [])),
    params(("id" = String, Path, description = "Session UUID")),
    responses(
        (status = 200, description = "Session deleted", body = DeleteSessionResponse),
        (status = 400, description = "Bad request", body = crate::error::ApiError),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError),
        (status = 404, description = "Session not found", body = crate::error::ApiError)
    )
)]
pub async fn delete_session(
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(plugin_ctx): Extension<PluginContext>,
    Path(id): Path<String>,
) -> Result<Json<DeleteSessionResponse>, crate::error::AppError> {
    // Parse UUID from path
    let session_id = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    // Remove session from store
    let session = store
        .remove(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    // If auto_workdir, remove the directory
    if session.auto_workdir {
        let _ = tokio::fs::remove_dir_all(&session.workdir).await;
    }

    // Emit session deleted event
    plugin_ctx.emit(GatewayEvent::SessionDeleted {
        session_id: session_id.to_string(),
    });

    Ok(Json(DeleteSessionResponse {
        deleted: true,
        session_id: session_id.to_string(),
    }))
}
