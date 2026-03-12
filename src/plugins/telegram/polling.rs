use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::claude;
use crate::config::Config;

use super::markdown;

/// Telegram Update object (subset of fields we care about).
#[derive(Debug, serde::Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TelegramMessage {
    pub chat: TelegramChat,
    pub text: Option<String>,
    pub message_thread_id: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TelegramChat {
    pub id: i64,
}

#[derive(Debug, serde::Deserialize)]
struct GetUpdatesResponse {
    ok: bool,
    result: Vec<TelegramUpdate>,
}

#[derive(Debug, serde::Deserialize)]
struct CreateForumTopicResponse {
    ok: bool,
    result: ForumTopicResult,
}

#[derive(Debug, serde::Deserialize)]
struct ForumTopicResult {
    message_thread_id: i64,
}

#[derive(Debug, serde::Deserialize)]
struct SendMessageResponse {
    ok: bool,
    result: Option<SentMessage>,
}

#[derive(Debug, serde::Deserialize)]
struct SentMessage {
    message_id: i64,
}

pub struct SessionInfo {
    pub claude_session_id: Option<String>,
    pub workdir: PathBuf,
    pub lock: Arc<tokio::sync::Mutex<()>>,
}

pub type SessionMap = Arc<tokio::sync::Mutex<HashMap<i64, SessionInfo>>>;

/// Send a text message to a Telegram chat, optionally in a specific thread.
/// Returns the message_id of the sent message.
pub async fn send_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: Option<i64>,
    text: &str,
) -> anyhow::Result<i64> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML"
    });
    if let Some(tid) = thread_id {
        body["message_thread_id"] = serde_json::json!(tid);
    }
    let response = client.post(&url).json(&body).send().await?;
    let parsed: SendMessageResponse = response.json().await?;
    if !parsed.ok {
        anyhow::bail!("sendMessage returned ok=false");
    }
    Ok(parsed.result.map(|r| r.message_id).unwrap_or(0))
}

/// Edit an existing message's text.
async fn edit_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    message_id: i64,
    text: &str,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/editMessageText");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
        "parse_mode": "HTML"
    });
    client.post(&url).json(&body).send().await?;
    Ok(())
}

/// Send typing indicator to a chat/thread.
async fn send_typing(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: Option<i64>,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendChatAction");
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "action": "typing"
    });
    if let Some(tid) = thread_id {
        body["message_thread_id"] = serde_json::json!(tid);
    }
    client.post(&url).json(&body).send().await?;
    Ok(())
}

/// Create a Forum Topic in the chat. Returns the new thread ID.
async fn create_forum_topic(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    name: &str,
) -> anyhow::Result<i64> {
    let url = format!("https://api.telegram.org/bot{bot_token}/createForumTopic");
    // Telegram allows 1-128 chars for topic name
    let truncated = if name.len() > 128 {
        format!("{}...", &name[..125])
    } else {
        name.to_string()
    };
    let body = serde_json::json!({
        "chat_id": chat_id,
        "name": truncated,
    });
    let response = client.post(&url).json(&body).send().await?;
    let parsed: CreateForumTopicResponse = response.json().await?;
    if !parsed.ok {
        anyhow::bail!("createForumTopic returned ok=false");
    }
    Ok(parsed.result.message_thread_id)
}

/// Delete a Forum Topic from the chat.
async fn delete_forum_topic(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: i64,
) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{bot_token}/deleteForumTopic");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_thread_id": thread_id,
    });
    client.post(&url).json(&body).send().await?;
    Ok(())
}

/// Long-poll Telegram for new messages and dispatch them to Claude.
pub async fn run_polling_loop(
    bot_token: String,
    chat_id: String,
    config: Arc<Config>,
    sessions: SessionMap,
) {
    let client = reqwest::Client::new();
    let mut offset: i64 = 0;

    tracing::info!("telegram polling loop running for chat_id={chat_id}");

    loop {
        let url = format!(
            "https://api.telegram.org/bot{bot_token}/getUpdates?timeout=30&offset={offset}"
        );

        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("telegram polling error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        let body: GetUpdatesResponse = match response.json().await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("telegram parse error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        if !body.ok {
            tracing::error!("telegram getUpdates returned ok=false");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        for update in body.result {
            offset = update.update_id + 1;

            let Some(message) = update.message else {
                continue;
            };
            let Some(text) = message.text else {
                continue;
            };

            // Security: only process messages from configured chat
            if message.chat.id.to_string() != chat_id {
                continue;
            }

            let thread_id = message.message_thread_id;
            let client = client.clone();
            let bot_token = bot_token.clone();
            let chat_id_clone = chat_id.clone();
            let config = config.clone();
            let sessions = sessions.clone();

            tokio::spawn(async move {
                handle_message(
                    &client,
                    &bot_token,
                    &chat_id_clone,
                    thread_id,
                    &text,
                    update.update_id,
                    &config,
                    &sessions,
                )
                .await;
            });
        }
    }
}

async fn handle_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: Option<i64>,
    prompt: &str,
    _update_id: i64,
    config: &Config,
    sessions: &SessionMap,
) {
    // Determine the topic thread_id — create a new topic if none
    let effective_thread_id = if let Some(tid) = thread_id {
        tid
    } else {
        // Create a new Forum Topic named after the first 50 chars of the message
        let topic_name = if prompt.len() > 50 {
            format!("{}...", &prompt[..50])
        } else {
            prompt.to_string()
        };
        match create_forum_topic(client, bot_token, chat_id, &topic_name).await {
            Ok(tid) => {
                tracing::info!("created forum topic {tid} for new message");
                tid
            }
            Err(e) => {
                tracing::error!("failed to create forum topic: {e}");
                let _ = send_message(client, bot_token, chat_id, None, &format!("Failed to create topic: {e}")).await;
                return;
            }
        }
    };

    // Handle /close command — delete topic and remove session
    if prompt.trim() == "/close" {
        {
            let mut map = sessions.lock().await;
            map.remove(&effective_thread_id);
        }
        let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id), "Session closed.").await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Err(e) = delete_forum_topic(client, bot_token, chat_id, effective_thread_id).await {
            tracing::error!("failed to delete forum topic: {e}");
        }
        return;
    }

    // Start typing indicator (runs until first streamed message or claude finishes)
    let typing_client = client.clone();
    let typing_token = bot_token.to_string();
    let typing_chat = chat_id.to_string();
    let typing_handle = tokio::spawn(async move {
        loop {
            let _ = send_typing(&typing_client, &typing_token, &typing_chat, Some(effective_thread_id)).await;
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
    });

    // Create streaming channel
    let (text_tx, text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Spawn streaming update handler (sends/edits Telegram messages as text arrives)
    let stream_client = client.clone();
    let stream_token = bot_token.to_string();
    let stream_chat = chat_id.to_string();
    let stream_handle = tokio::spawn(async move {
        handle_streaming_updates(
            &stream_client,
            &stream_token,
            &stream_chat,
            effective_thread_id,
            text_rx,
        )
        .await
    });

    // Run Claude with streaming
    let result = handle_topic_message(prompt, effective_thread_id, config, sessions, text_tx).await;

    // Stop typing indicator
    typing_handle.abort();

    // Wait for streaming handler to finish, get message IDs it created
    let sent_message_ids = stream_handle.await.unwrap_or_default();

    let response_text = match result {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    };

    // Final update — ensure complete text is displayed correctly
    let chunks = markdown::split_and_convert(&response_text);
    for (i, chunk) in chunks.iter().enumerate() {
        if i < sent_message_ids.len() {
            // Edit existing streaming message with final content
            let _ = edit_message(client, bot_token, chat_id, sent_message_ids[i], chunk).await;
        } else {
            // Send additional messages for remaining chunks
            let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id), chunk).await;
        }
    }
}

/// Process streaming text updates from Claude and push them to Telegram via send/edit.
/// Returns the message IDs of all messages created during streaming.
async fn handle_streaming_updates(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: i64,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<String>,
) -> Vec<i64> {
    let mut message_ids: Vec<i64> = Vec::new();
    let mut last_text = String::new();
    let debounce = Duration::from_millis(1000);

    while let Some(text) = rx.recv().await {
        // Drain channel to get the latest accumulated text
        let mut latest = text;
        while let Ok(newer) = rx.try_recv() {
            latest = newer;
        }

        if latest == last_text {
            continue;
        }
        last_text.clone_from(&latest);

        let chunks = markdown::split_and_convert(&latest);

        for (i, chunk) in chunks.iter().enumerate() {
            if i < message_ids.len() {
                // Only edit the last (still-growing) chunk
                if i == chunks.len() - 1 {
                    let _ = edit_message(client, bot_token, chat_id, message_ids[i], chunk).await;
                }
            } else {
                // New chunk needed — send a new message
                match send_message(client, bot_token, chat_id, Some(thread_id), chunk) .await
                {
                    Ok(mid) => message_ids.push(mid),
                    Err(e) => tracing::error!("failed to send streaming chunk: {e}"),
                }
            }
        }

        // Debounce to avoid Telegram rate limits
        tokio::time::sleep(debounce).await;
    }

    message_ids
}

async fn handle_topic_message(
    prompt: &str,
    thread_id: i64,
    config: &Config,
    sessions: &SessionMap,
    text_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> anyhow::Result<String> {
    // Get or create session + acquire per-topic lock
    let topic_lock = {
        let mut map = sessions.lock().await;
        let session = map.entry(thread_id).or_insert_with(|| {
            let workdir = PathBuf::from(&config.claude_workdir)
                .join("telegram")
                .join(thread_id.to_string());
            SessionInfo {
                claude_session_id: None,
                workdir,
                lock: Arc::new(tokio::sync::Mutex::new(())),
            }
        });
        session.lock.clone()
    };

    let _guard = topic_lock.lock().await;

    // Re-lock map to read current session state
    let (claude_session_id, workdir) = {
        let map = sessions.lock().await;
        let session = map.get(&thread_id).unwrap();
        (session.claude_session_id.clone(), session.workdir.clone())
    };

    tokio::fs::create_dir_all(&workdir).await?;

    let claude_result = if let Some(ref sid) = claude_session_id {
        claude::run_resume_streaming(config, prompt, sid, &workdir, 600, text_tx).await
    } else {
        claude::run_task_streaming(config, prompt, None, None, &workdir, 600, text_tx).await
    };

    match claude_result {
        Ok(result) => {
            // Store claude_session_id if we got one
            if result.claude_session_id.is_some() {
                let mut map = sessions.lock().await;
                if let Some(session) = map.get_mut(&thread_id) {
                    if session.claude_session_id.is_none() {
                        session.claude_session_id = result.claude_session_id;
                    }
                }
            }
            Ok(result
                .result
                .unwrap_or_else(|| "No response from Claude.".to_string()))
        }
        Err(e) => {
            let err_msg = e.body.error;
            if err_msg.contains("timed out") {
                Ok("Claude did not respond in time. Try again.".to_string())
            } else {
                Ok(format!("Claude error: {err_msg}"))
            }
        }
    }
}

