use axum::extract::Extension;
use axum::Json;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::KeyId;
use crate::claude;
use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::logging::{ClaudeInvocationLog, KeyLogger};
use crate::models::{TaskRequest, TaskResponse};
use crate::plugin::{GatewayEvent, PluginContext};

const DEFAULT_TIMEOUT: u64 = 120;

#[utoipa::path(
    post,
    path = "/task",
    tag = "Tasks",
    summary = "Run a one-shot task",
    description = "Execute a single Claude prompt with no session persistence. Returns result with token usage and cost.",
    security(("bearer" = [])),
    request_body = TaskRequest,
    responses(
        (status = 200, description = "Task completed", body = TaskResponse),
        (status = 400, description = "Bad request", body = crate::error::ApiError),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError),
        (status = 408, description = "Timeout", body = crate::error::ApiError),
        (status = 500, description = "Internal error", body = crate::error::ApiError)
    )
)]
pub async fn create_task(
    Extension(key_id): Extension<KeyId>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Extension(plugin_ctx): Extension<PluginContext>,
    Json(req): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    request_counter.fetch_add(1, Ordering::Relaxed);

    // 1. Validate prompt is not empty
    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt cannot be empty"));
    }

    // 2. Resolve workdir: req.workdir or config.claude_workdir
    let workdir = if let Some(wd) = &req.workdir {
        PathBuf::from(wd)
    } else {
        PathBuf::from(&config.claude_workdir)
    };

    // 3. Create workdir via tokio::fs::create_dir_all
    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create workdir: {}", e)))?;

    // 4. Generate UUID for this task
    let session_id = Uuid::new_v4().to_string();

    // 5. Call claude::run_task with config, prompt, model, system_prompt, workdir, timeout
    let timeout_secs = req.timeout_secs.unwrap_or(DEFAULT_TIMEOUT);
    let claude_result = claude::run_task(
        &config,
        &req.prompt,
        req.model.as_deref(),
        req.system_prompt.as_deref(),
        &workdir,
        timeout_secs,
    )
    .await?;

    // 6. Log the invocation via logger.log_claude_invocation
    let log_entry = ClaudeInvocationLog {
        timestamp: Utc::now().to_rfc3339(),
        level: "INFO",
        key_id: key_id.0.clone(),
        session_id: session_id.clone(),
        model: req.model.clone(),
        exit_code: claude_result.exit_code,
        duration_ms: claude_result.duration_ms,
        success: claude_result.success,
        tokens: claude_result.tokens.clone(),
        cost_usd: claude_result.cost_usd,
        message: format!(
            "Task {} completed with status: {}",
            session_id,
            if claude_result.success {
                "success"
            } else {
                "failure"
            }
        ),
    };
    logger.log_claude_invocation(&log_entry);

    // Emit cost event
    if let Some(cost) = claude_result.cost_usd {
        plugin_ctx.emit(GatewayEvent::CostRecorded {
            key_id: key_id.0.clone(),
            model: req.model.clone().unwrap_or_else(|| "unknown".to_string()),
            cost,
        });
    }

    // 7. Return TaskResponse with all fields populated
    let error = if claude_result.success {
        None
    } else {
        Some("Claude CLI returned an error".to_string())
    };

    Ok(Json(TaskResponse {
        session_id,
        result: claude_result.result,
        success: claude_result.success,
        duration_ms: claude_result.duration_ms,
        tokens: claude_result.tokens,
        cost_usd: claude_result.cost_usd,
        error,
        permission_denials: claude_result.permission_denials,
    }))
}
