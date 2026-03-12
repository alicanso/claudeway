use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::Extension;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::auth::KeyId;
use crate::claude;
use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::logging::{ClaudeInvocationLog, KeyLogger};
use crate::models::{TaskRequest, TaskResponse};
use crate::plugin::{GatewayEvent, PluginContext};

#[utoipa::path(
    post,
    path = "/task/stream",
    tag = "Tasks",
    summary = "Run a task with streaming",
    description = "Execute a Claude prompt and stream the response via Server-Sent Events. Sends 'text' events with partial content and a final 'done' event with the complete TaskResponse.",
    security(("bearer" = [])),
    request_body = TaskRequest,
    responses(
        (status = 200, description = "SSE stream of task progress"),
        (status = 400, description = "Bad request", body = crate::error::ApiError),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError)
    )
)]
pub async fn create_task_stream(
    Extension(key_id): Extension<KeyId>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Extension(plugin_ctx): Extension<PluginContext>,
    Json(req): Json<TaskRequest>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, AppError> {
    request_counter.fetch_add(1, Ordering::Relaxed);

    if req.prompt.trim().is_empty() {
        return Err(ApiError::bad_request("prompt cannot be empty"));
    }

    let workdir = if let Some(wd) = &req.workdir {
        PathBuf::from(wd)
    } else {
        PathBuf::from(&config.claude_workdir)
    };

    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create workdir: {}", e)))?;

    let session_id = Uuid::new_v4().to_string();
    let timeout_secs = req.timeout_secs.unwrap_or(120);

    // Channel for streaming text from Claude
    let (text_tx, text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Channel for SSE events to the client
    let (sse_tx, sse_rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();

    // Spawn the Claude streaming task
    let sse_tx_done = sse_tx.clone();
    let config_clone = config.clone();
    let session_id_clone = session_id.clone();
    let key_id_clone = key_id.0.clone();
    tokio::spawn(async move {
        let sse_tx = sse_tx_done;
        let claude_result = claude::run_task_streaming(
            &config_clone,
            &req.prompt,
            req.model.as_deref(),
            req.system_prompt.as_deref(),
            &workdir,
            timeout_secs,
            text_tx,
        )
        .await;

        match claude_result {
            Ok(result) => {
                // Log invocation
                let log_entry = ClaudeInvocationLog {
                    timestamp: Utc::now().to_rfc3339(),
                    level: "INFO",
                    key_id: key_id_clone.clone(),
                    session_id: session_id_clone.clone(),
                    model: req.model.clone(),
                    exit_code: result.exit_code,
                    duration_ms: result.duration_ms,
                    success: result.success,
                    tokens: result.tokens.clone(),
                    cost_usd: result.cost_usd,
                    message: format!("Streaming task {} completed", session_id_clone),
                };
                logger.log_claude_invocation(&log_entry);

                // Emit cost event
                if let Some(cost) = result.cost_usd {
                    plugin_ctx.emit(GatewayEvent::CostRecorded {
                        key_id: key_id_clone,
                        model: req.model.unwrap_or_else(|| "unknown".to_string()),
                        cost,
                    });
                }

                // Emit permission_denied event if needed
                if !result.permission_denials.is_empty() {
                    let denials_json = serde_json::to_string(&result.permission_denials).unwrap_or_default();
                    let _ = sse_tx.send(Ok(Event::default()
                        .event("permission_denied")
                        .data(denials_json)));
                }

                // Send final "done" event with complete TaskResponse
                let response = TaskResponse {
                    session_id: session_id_clone,
                    result: result.result,
                    success: result.success,
                    duration_ms: result.duration_ms,
                    tokens: result.tokens,
                    cost_usd: result.cost_usd,
                    error: if result.success {
                        None
                    } else {
                        Some("Claude CLI returned an error".to_string())
                    },
                    permission_denials: result.permission_denials,
                };
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = sse_tx.send(Ok(Event::default().event("done").data(json)));
            }
            Err(e) => {
                let error_response = TaskResponse {
                    session_id: session_id_clone,
                    result: None,
                    success: false,
                    duration_ms: 0,
                    tokens: None,
                    cost_usd: None,
                    error: Some(e.body.error),
                    permission_denials: Vec::new(),
                };
                let json = serde_json::to_string(&error_response).unwrap_or_default();
                let _ = sse_tx.send(Ok(Event::default().event("done").data(json)));
            }
        }
    });

    // Spawn a task to forward text updates as SSE events
    let sse_tx_text = sse_tx.clone();
    tokio::spawn(async move {
        let mut rx_stream = UnboundedReceiverStream::new(text_rx);
        while let Some(text) = rx_stream.next().await {
            let event = Event::default().event("text").data(text);
            if sse_tx_text.send(Ok(event)).is_err() {
                break; // Client disconnected
            }
        }
    });

    let stream = UnboundedReceiverStream::new(sse_rx);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[utoipa::path(
    get,
    path = "/task/ws",
    tag = "Tasks",
    summary = "Run a task with WebSocket streaming",
    description = "Upgrade to a WebSocket connection for bidirectional streaming. Send a TaskRequest JSON as the first message. The server streams back `{\"event\":\"text\",\"data\":\"...\"}` messages with partial content, and a final `{\"event\":\"done\",\"data\":{TaskResponse}}` message before closing.",
    security(("bearer" = [])),
    responses(
        (status = 101, description = "WebSocket upgrade"),
        (status = 401, description = "Unauthorized", body = crate::error::ApiError)
    )
)]
pub async fn create_task_ws(
    ws: WebSocketUpgrade,
    Extension(key_id): Extension<KeyId>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Extension(plugin_ctx): Extension<PluginContext>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        handle_ws(socket, key_id, request_counter, config, logger, plugin_ctx)
    })
}

async fn handle_ws(
    mut socket: WebSocket,
    key_id: KeyId,
    request_counter: Arc<AtomicU64>,
    config: Arc<Config>,
    logger: Arc<KeyLogger>,
    plugin_ctx: PluginContext,
) {
    // Wait for the first message: TaskRequest JSON
    let req: TaskRequest = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str(&text) {
                    Ok(r) => break r,
                    Err(e) => {
                        let _ = socket
                            .send(Message::Text(
                                serde_json::json!({"event":"error","data":format!("Invalid request: {e}")}).to_string().into(),
                            ))
                            .await;
                        return;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue, // skip ping/pong/binary
        }
    };

    request_counter.fetch_add(1, Ordering::Relaxed);

    if req.prompt.trim().is_empty() {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"event":"error","data":"prompt cannot be empty"}).to_string().into(),
            ))
            .await;
        return;
    }

    let workdir = if let Some(wd) = &req.workdir {
        PathBuf::from(wd)
    } else {
        PathBuf::from(&config.claude_workdir)
    };

    if tokio::fs::create_dir_all(&workdir).await.is_err() {
        let _ = socket
            .send(Message::Text(
                serde_json::json!({"event":"error","data":"Failed to create workdir"}).to_string().into(),
            ))
            .await;
        return;
    }

    let session_id = Uuid::new_v4().to_string();
    let timeout_secs = req.timeout_secs.unwrap_or(120);

    // Channel for streaming text from Claude
    let (text_tx, mut text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Spawn Claude streaming task
    let config_clone = config.clone();
    let session_id_clone = session_id.clone();
    let key_id_clone = key_id.0.clone();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<TaskResponse>();

    tokio::spawn(async move {
        let claude_result = claude::run_task_streaming(
            &config_clone,
            &req.prompt,
            req.model.as_deref(),
            req.system_prompt.as_deref(),
            &workdir,
            timeout_secs,
            text_tx,
        )
        .await;

        let response = match claude_result {
            Ok(result) => {
                let log_entry = ClaudeInvocationLog {
                    timestamp: Utc::now().to_rfc3339(),
                    level: "INFO",
                    key_id: key_id_clone.clone(),
                    session_id: session_id_clone.clone(),
                    model: req.model.clone(),
                    exit_code: result.exit_code,
                    duration_ms: result.duration_ms,
                    success: result.success,
                    tokens: result.tokens.clone(),
                    cost_usd: result.cost_usd,
                    message: format!("WS streaming task {} completed", session_id_clone),
                };
                logger.log_claude_invocation(&log_entry);

                if let Some(cost) = result.cost_usd {
                    plugin_ctx.emit(GatewayEvent::CostRecorded {
                        key_id: key_id_clone,
                        model: req.model.unwrap_or_else(|| "unknown".to_string()),
                        cost,
                    });
                }

                TaskResponse {
                    session_id: session_id_clone,
                    result: result.result,
                    success: result.success,
                    duration_ms: result.duration_ms,
                    tokens: result.tokens,
                    cost_usd: result.cost_usd,
                    error: if result.success {
                        None
                    } else {
                        Some("Claude CLI returned an error".to_string())
                    },
                    permission_denials: result.permission_denials,
                }
            }
            Err(e) => TaskResponse {
                session_id: session_id_clone,
                result: None,
                success: false,
                duration_ms: 0,
                tokens: None,
                cost_usd: None,
                error: Some(e.body.error),
                permission_denials: Vec::new(),
            },
        };

        let _ = done_tx.send(response);
    });

    // Forward streaming text updates via WebSocket
    let done_rx = done_rx;
    loop {
        tokio::select! {
            text = text_rx.recv() => {
                match text {
                    Some(t) => {
                        let msg = serde_json::json!({"event": "text", "data": t});
                        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                            return; // Client disconnected
                        }
                    }
                    None => break, // Stream ended
                }
            }
            _ = socket.recv() => {
                // Client sent close or disconnected — we still finish but stop sending
                return;
            }
        }
    }

    // Send final done event
    if let Ok(response) = done_rx.await {
        let msg = serde_json::json!({"event": "done", "data": response});
        let _ = socket.send(Message::Text(msg.to_string().into())).await;
    }

}
