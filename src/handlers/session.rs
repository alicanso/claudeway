use axum::extract::{Extension, Path};
use axum::Json;
use chrono::Utc;
use std::path::PathBuf;
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
use crate::session::{SessionMeta, SessionStore};

/// Start a new session
pub async fn start_session(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
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
    };

    // Insert into store
    store.insert(meta);

    Ok(Json(SessionStartResponse {
        session_id: session_id.to_string(),
        workdir: workdir.to_string_lossy().to_string(),
        created_at,
    }))
}

/// Continue an existing session with a new prompt
pub async fn continue_session(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(logger): Extension<Arc<crate::logging::KeyLogger>>,
    Path(id): Path<String>,
    Json(req): Json<SessionContinueRequest>,
) -> Result<Json<TaskResponse>, crate::error::AppError> {
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

/// Get session information
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

/// Delete a session
pub async fn delete_session(
    Extension(store): Extension<Arc<SessionStore>>,
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

    Ok(Json(DeleteSessionResponse {
        deleted: true,
        session_id: session_id.to_string(),
    }))
}
