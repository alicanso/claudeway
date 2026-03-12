use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::claude;
use crate::config::Config;

use super::markdown;
use super::repos::{self, RepoInfo};

/// Telegram Update object (subset of fields we care about).
#[derive(Debug, serde::Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub data: Option<String>,
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TelegramMessage {
    #[serde(default)]
    pub message_id: i64,
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

#[derive(Debug)]
pub enum TopicState {
    AwaitingRepoSelection {
        pending_prompt: String,
        repos: Vec<RepoInfo>,
        page: usize,
    },
    Active,
}

pub struct SessionInfo {
    pub claude_session_id: Option<String>,
    pub workdir: PathBuf,
    pub lock: Arc<tokio::sync::Mutex<()>>,
    pub state: TopicState,
    pub pending_approval: Option<tokio::sync::oneshot::Sender<bool>>,
}

pub type SessionMap = Arc<tokio::sync::Mutex<HashMap<i64, SessionInfo>>>;

/// Serializable subset of SessionInfo for disk persistence.
#[derive(Serialize, Deserialize)]
struct PersistedSession {
    claude_session_id: Option<String>,
    workdir: PathBuf,
}

/// Save active sessions to a JSON file. Only persists sessions with Active state.
async fn save_sessions(sessions: &SessionMap, path: &std::path::Path) {
    let map = sessions.lock().await;
    let persisted: HashMap<String, PersistedSession> = map
        .iter()
        .filter(|(_, s)| matches!(s.state, TopicState::Active))
        .map(|(tid, s)| {
            (
                tid.to_string(),
                PersistedSession {
                    claude_session_id: s.claude_session_id.clone(),
                    workdir: s.workdir.clone(),
                },
            )
        })
        .collect();
    drop(map);

    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let tmp = path.with_extension("tmp");
    match serde_json::to_string_pretty(&persisted) {
        Ok(json) => {
            if let Err(e) = tokio::fs::write(&tmp, &json).await {
                tracing::error!("failed to write sessions file: {e}");
                return;
            }
            if let Err(e) = tokio::fs::rename(&tmp, path).await {
                tracing::error!("failed to rename sessions file: {e}");
            }
        }
        Err(e) => tracing::error!("failed to serialize sessions: {e}"),
    }
}

/// Load persisted sessions from a JSON file into the SessionMap.
async fn load_sessions(sessions: &SessionMap, path: &std::path::Path) {
    let data = match tokio::fs::read_to_string(path).await {
        Ok(d) => d,
        Err(_) => return, // File doesn't exist yet — first run
    };

    let persisted: HashMap<String, PersistedSession> = match serde_json::from_str(&data) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("failed to parse sessions file, starting fresh: {e}");
            return;
        }
    };

    let mut map = sessions.lock().await;
    for (tid_str, ps) in persisted {
        let Ok(tid) = tid_str.parse::<i64>() else {
            continue;
        };
        map.insert(
            tid,
            SessionInfo {
                claude_session_id: ps.claude_session_id,
                workdir: ps.workdir,
                lock: Arc::new(tokio::sync::Mutex::new(())),
                state: TopicState::Active,
                pending_approval: None,
            },
        );
    }

    tracing::info!("loaded {} telegram sessions from disk", map.len());
}

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

async fn answer_callback_query(client: &reqwest::Client, bot_token: &str, callback_query_id: &str, text: &str) {
    let url = format!("https://api.telegram.org/bot{bot_token}/answerCallbackQuery");
    let body = serde_json::json!({
        "callback_query_id": callback_query_id,
        "text": text,
    });
    let _ = client.post(&url).json(&body).send().await;
}

async fn send_message_with_keyboard(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: i64,
    text: &str,
    keyboard: serde_json::Value,
) -> anyhow::Result<i64> {
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_thread_id": thread_id,
        "text": text,
        "parse_mode": "HTML",
        "reply_markup": keyboard,
    });
    let resp = client.post(&url).json(&body).send().await?;
    let json: serde_json::Value = resp.json().await?;
    json.get("result")
        .and_then(|r| r.get("message_id"))
        .and_then(|m| m.as_i64())
        .ok_or_else(|| anyhow::anyhow!("No message_id in response"))
}

async fn remove_keyboard(client: &reqwest::Client, bot_token: &str, chat_id: &str, message_id: i64) {
    let url = format!("https://api.telegram.org/bot{bot_token}/editMessageReplyMarkup");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "reply_markup": {"inline_keyboard": []},
    });
    let _ = client.post(&url).json(&body).send().await;
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
    projects_dir: PathBuf,
) {
    let client = reqwest::Client::new();
    let mut offset: i64 = 0;

    // Persistence file path
    let sessions_file = PathBuf::from(&config.claude_workdir)
        .join("telegram")
        .join("sessions.json");

    // Load persisted sessions from previous runs
    load_sessions(&sessions, &sessions_file).await;

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

            // Handle callback queries (permission approval buttons)
            if let Some(callback) = update.callback_query {
                let bot_token = bot_token.clone();
                let chat_id = chat_id.clone();
                let sessions = sessions.clone();
                let client = client.clone();

                tokio::spawn(async move {
                    handle_callback_query(&client, &bot_token, &chat_id, callback, sessions).await;
                });
                continue;
            }

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
            let projects_dir = projects_dir.clone();

            let sessions_file = sessions_file.clone();
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
                    &projects_dir,
                    &sessions_file,
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
    projects_dir: &std::path::Path,
    sessions_file: &std::path::Path,
) {
    // Determine the topic thread_id — handle repo selection flow
    // Check session state for existing thread_ids
    let session_state = if let Some(tid) = thread_id {
        let map = sessions.lock().await;
        match map.get(&tid) {
            Some(s) if matches!(s.state, TopicState::AwaitingRepoSelection { .. }) => {
                Some((tid, "awaiting"))
            }
            Some(_) => Some((tid, "active")),
            None => Some((tid, "new")),
        }
    } else {
        None
    };

    let (effective_thread_id, prompt) = match session_state {
        // Existing session awaiting repo selection
        Some((tid, "awaiting")) => {
            if let Some(pending_prompt) =
                handle_repo_selection(client, bot_token, chat_id, tid, prompt, sessions, projects_dir).await
            {
                save_sessions(sessions, sessions_file).await;
                (tid, std::borrow::Cow::Owned(pending_prompt))
            } else {
                return;
            }
        }
        // Existing active session — pass through to Claude
        Some((tid, "active")) => (tid, std::borrow::Cow::Borrowed(prompt)),
        // Thread exists but no session (e.g. General topic) or no thread at all — start repo selection
        _ => {
            // Reuse existing topic if message came from one, otherwise create new
            let tid = if let Some(existing_tid) = thread_id {
                existing_tid
            } else {
                let topic_name = "Selecting repo...";
                match create_forum_topic(client, bot_token, chat_id, topic_name).await {
                    Ok(tid) => tid,
                    Err(e) => {
                        let _ = send_message(
                            client,
                            bot_token,
                            chat_id,
                            None,
                            &format!("Failed to create topic: {e}"),
                        )
                        .await;
                        return;
                    }
                }
            };

            // Reserve session BEFORE discover_repos to prevent race conditions
            // (another message arriving during discovery would see "awaiting" state)
            {
                let mut map = sessions.lock().await;
                map.insert(
                    tid,
                    SessionInfo {
                        claude_session_id: None,
                        workdir: PathBuf::new(),
                        lock: Arc::new(tokio::sync::Mutex::new(())),
                        pending_approval: None,
                        state: TopicState::AwaitingRepoSelection {
                            pending_prompt: prompt.to_string(),
                            repos: Vec::new(), // placeholder until discovery completes
                            page: 0,
                        },
                    },
                );
            }

            let repos = match repos::discover_repos().await {
                Ok(r) if r.is_empty() => {
                    let _ = send_message(client, bot_token, chat_id, Some(tid), "No repositories found.")
                        .await;
                    // Clean up placeholder session
                    let mut map = sessions.lock().await;
                    map.remove(&tid);
                    return;
                }
                Ok(r) => r,
                Err(e) => {
                    let _ = send_message(
                        client,
                        bot_token,
                        chat_id,
                        Some(tid),
                        &format!("Failed to list repos: {e}"),
                    )
                    .await;
                    let mut map = sessions.lock().await;
                    map.remove(&tid);
                    return;
                }
            };

            // Send repo list
            let (msg, _) = repos::format_repo_page(&repos, 0, 20);
            let _ = send_message(client, bot_token, chat_id, Some(tid), &msg).await;

            // Update session with discovered repos
            {
                let mut map = sessions.lock().await;
                if let Some(session) = map.get_mut(&tid) {
                    session.state = TopicState::AwaitingRepoSelection {
                        pending_prompt: prompt.to_string(),
                        repos,
                        page: 0,
                    };
                }
            }
            return; // Don't proceed to Claude yet
        }
    };

    let prompt = &*prompt;

    // Handle /close command — delete topic and remove session
    if prompt.trim() == "/close" {
        {
            let mut map = sessions.lock().await;
            map.remove(&effective_thread_id);
        }
        save_sessions(sessions, sessions_file).await;
        let _ = send_message(
            client,
            bot_token,
            chat_id,
            Some(effective_thread_id),
            "Session closed.",
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Err(e) = delete_forum_topic(client, bot_token, chat_id, effective_thread_id).await {
            tracing::error!("failed to delete forum topic: {e}");
        }
        return;
    }

    // Handle /repos command — re-trigger repo selection in this topic
    if prompt.trim() == "/repos" {
        let repos = match repos::discover_repos().await {
            Ok(r) => r,
            Err(e) => {
                let _ = send_message(
                    client,
                    bot_token,
                    chat_id,
                    Some(effective_thread_id),
                    &format!("Failed to list repos: {e}"),
                )
                .await;
                return;
            }
        };
        let (msg, _) = repos::format_repo_page(&repos, 0, 20);
        let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id), &msg).await;
        {
            let mut map = sessions.lock().await;
            if let Some(session) = map.get_mut(&effective_thread_id) {
                session.state = TopicState::AwaitingRepoSelection {
                    pending_prompt: String::new(),
                    repos,
                    page: 0,
                };
            }
        }
        return;
    }

    // Start typing indicator (runs until first streamed message or claude finishes)
    let typing_client = client.clone();
    let typing_token = bot_token.to_string();
    let typing_chat = chat_id.to_string();
    let typing_handle = tokio::spawn(async move {
        loop {
            let _ = send_typing(
                &typing_client,
                &typing_token,
                &typing_chat,
                Some(effective_thread_id),
            )
            .await;
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
    let result =
        handle_topic_message(prompt, effective_thread_id, config, sessions, text_tx).await;

    // Stop typing indicator
    typing_handle.abort();

    // Wait for streaming handler to finish, get message IDs it created
    let sent_message_ids = stream_handle.await.unwrap_or_default();

    // Persist sessions after Claude interaction (session_id may have been set)
    save_sessions(sessions, sessions_file).await;

    let (response_text, permission_denials) = match result {
        Ok(claude_result) => {
            let text = claude_result.result.clone().unwrap_or_else(|| "No response from Claude.".to_string());
            (text, claude_result.permission_denials)
        }
        Err(e) => {
            let err_msg = format!("{e}");
            if err_msg.contains("timed out") {
                ("Claude did not respond in time. Try again.".to_string(), Vec::new())
            } else {
                (format!("Claude error: {err_msg}"), Vec::new())
            }
        }
    };

    // Final update — ensure complete text is displayed correctly
    let chunks = markdown::split_and_convert(&response_text);
    for (i, chunk) in chunks.iter().enumerate() {
        if i < sent_message_ids.len() {
            // Edit existing streaming message with final content
            let _ = edit_message(client, bot_token, chat_id, sent_message_ids[i], chunk).await;
        } else {
            // Send additional messages for remaining chunks
            let _ = send_message(
                client,
                bot_token,
                chat_id,
                Some(effective_thread_id),
                chunk,
            )
            .await;
        }
    }

    // Handle permission denials with inline keyboard
    if !permission_denials.is_empty() {
        let mut denial_text = String::from("<b>Claude izin istiyor:</b>\n\n");
        for denial in &permission_denials {
            let detail = denial.tool_input.get("file_path")
                .and_then(|p| p.as_str())
                .or_else(|| denial.tool_input.get("command").and_then(|c| c.as_str()))
                .unwrap_or("(detay tool_input'ta)");
            denial_text.push_str(&format!("<code>{}</code>: {}\n", denial.tool_name, detail));
        }

        let keyboard = serde_json::json!({
            "inline_keyboard": [[
                {"text": "Onayla", "callback_data": format!("perm_approve:{}", effective_thread_id)},
                {"text": "Reddet", "callback_data": format!("perm_deny:{}", effective_thread_id)}
            ]]
        });

        let kb_msg_id = send_message_with_keyboard(
            client, bot_token, chat_id, effective_thread_id, &denial_text, keyboard,
        ).await.ok();

        // Set up approval channel
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        {
            let mut sessions_guard = sessions.lock().await;
            if let Some(session) = sessions_guard.get_mut(&effective_thread_id) {
                session.pending_approval = Some(tx);
            }
        }

        // Wait for approval with 60s timeout
        let approved = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            rx,
        ).await.unwrap_or(Ok(false)).unwrap_or(false);

        // Remove keyboard buttons
        if let Some(msg_id) = kb_msg_id {
            remove_keyboard(client, bot_token, chat_id, msg_id).await;
        }

        if approved {
            let session_data = {
                let sessions_guard = sessions.lock().await;
                sessions_guard.get(&effective_thread_id).map(|s| (
                    s.claude_session_id.clone(),
                    s.workdir.clone(),
                ))
            };

            if let Some((Some(sid), workdir)) = session_data {
                // Start typing indicator for retry
                let typing_client = client.clone();
                let typing_token = bot_token.to_string();
                let typing_chat = chat_id.to_string();
                let typing_handle = tokio::spawn(async move {
                    loop {
                        let _ = send_typing(
                            &typing_client,
                            &typing_token,
                            &typing_chat,
                            Some(effective_thread_id),
                        ).await;
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    }
                });

                let (text_tx, text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

                // Spawn streaming update handler for retry
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
                    ).await
                });

                let retry_result = claude::run_resume_streaming(
                    config,
                    "Please proceed with the previously denied tool calls.",
                    &sid,
                    &workdir,
                    600,
                    true, // force_bypass
                    text_tx,
                ).await;

                typing_handle.abort();
                let retry_msg_ids = stream_handle.await.unwrap_or_default();

                match retry_result {
                    Ok(result) => {
                        // Store updated session_id
                        if let Some(ref new_sid) = result.claude_session_id {
                            let mut map = sessions.lock().await;
                            if let Some(session) = map.get_mut(&effective_thread_id) {
                                session.claude_session_id = Some(new_sid.clone());
                            }
                        }
                        save_sessions(sessions, sessions_file).await;
                        let text = result.result.unwrap_or_else(|| "Done.".to_string());
                        let chunks = markdown::split_and_convert(&text);
                        for (i, chunk) in chunks.iter().enumerate() {
                            if i < retry_msg_ids.len() {
                                let _ = edit_message(client, bot_token, chat_id, retry_msg_ids[i], chunk).await;
                            } else {
                                let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id), chunk).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id),
                            &format!("Hata: {:?}", e)).await;
                    }
                }
            }
        } else {
            let _ = send_message(client, bot_token, chat_id, Some(effective_thread_id),
                "Permission denied.").await;
        }
    }
}

/// Handle user input during repo selection (number, /next, /prev).
/// Returns `Some(pending_prompt)` when a repo is selected and Claude should proceed.
async fn handle_repo_selection(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    thread_id: i64,
    input: &str,
    sessions: &SessionMap,
    projects_dir: &std::path::Path,
) -> Option<String> {
    let input = input.trim();

    // Handle /next and /prev commands
    if input == "/next" || input == "/prev" {
        let mut map = sessions.lock().await;
        if let Some(session) = map.get_mut(&thread_id) {
            if let TopicState::AwaitingRepoSelection { repos, page, .. } = &mut session.state {
                let per_page = 20;
                if input == "/next" {
                    let max_page = repos.len().saturating_sub(1) / per_page;
                    if *page < max_page {
                        *page += 1;
                    }
                } else if *page > 0 {
                    *page -= 1;
                }
                let (msg, _) = repos::format_repo_page(repos, *page, per_page);
                let _ = send_message(client, bot_token, chat_id, Some(thread_id), &msg).await;
            }
        }
        return None;
    }

    // Check if repos are still loading
    {
        let map = sessions.lock().await;
        if let Some(session) = map.get(&thread_id) {
            if let TopicState::AwaitingRepoSelection { repos, .. } = &session.state {
                if repos.is_empty() {
                    let _ = send_message(
                        client,
                        bot_token,
                        chat_id,
                        Some(thread_id),
                        "Still loading repositories, please wait...",
                    )
                    .await;
                    return None;
                }
            }
        }
    }

    // Parse number selection
    let Ok(num) = input.parse::<usize>() else {
        let _ = send_message(
            client,
            bot_token,
            chat_id,
            Some(thread_id),
            "Please send a number to select a repo, /next for more, or /prev to go back.",
        )
        .await;
        return None;
    };

    // Extract repo info and pending prompt
    let (repo, pending_prompt) = {
        let map = sessions.lock().await;
        let session = map.get(&thread_id)?;
        let TopicState::AwaitingRepoSelection {
            repos,
            pending_prompt,
            ..
        } = &session.state
        else {
            return None;
        };

        if num == 0 || num > repos.len() {
            let _ = send_message(
                client,
                bot_token,
                chat_id,
                Some(thread_id),
                &format!("Invalid number. Choose 1-{}.", repos.len()),
            )
            .await;
            return None;
        }
        (repos[num - 1].clone(), pending_prompt.clone())
    };

    // Clone or pull the repo
    let _ = send_message(
        client,
        bot_token,
        chat_id,
        Some(thread_id),
        &format!("Setting up {}...", repo.full_name),
    )
    .await;

    let repo_path = match repos::ensure_repo(&repo, projects_dir).await {
        Ok(p) => p,
        Err(e) => {
            let _ = send_message(
                client,
                bot_token,
                chat_id,
                Some(thread_id),
                &format!("Failed to set up repo: {e}"),
            )
            .await;
            return None;
        }
    };

    // Rename the topic to the repo name
    let rename_url = format!("https://api.telegram.org/bot{bot_token}/editForumTopic");
    let rename_body = serde_json::json!({
        "chat_id": chat_id,
        "message_thread_id": thread_id,
        "name": &repo.full_name,
    });
    let _ = client.post(&rename_url).json(&rename_body).send().await;

    // Transition to Active state
    {
        let mut map = sessions.lock().await;
        if let Some(session) = map.get_mut(&thread_id) {
            session.workdir = repo_path;
            session.state = TopicState::Active;
        }
    }

    let _ = send_message(
        client,
        bot_token,
        chat_id,
        Some(thread_id),
        &format!("Using {}\n\nProcessing your prompt...", repo.full_name),
    )
    .await;

    if pending_prompt.is_empty() {
        None
    } else {
        Some(pending_prompt)
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

async fn handle_callback_query(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    callback: CallbackQuery,
    sessions: SessionMap,
) {
    let Some(data) = callback.data else { return };

    let parts: Vec<&str> = data.splitn(2, ':').collect();
    if parts.len() != 2 { return }

    let (action, thread_id_str) = (parts[0], parts[1]);
    let Ok(thread_id) = thread_id_str.parse::<i64>() else { return };

    let approved = action == "perm_approve";

    // Take the pending approval sender
    let sender = {
        let mut sessions_guard = sessions.lock().await;
        sessions_guard.get_mut(&thread_id)
            .and_then(|s| s.pending_approval.take())
    };

    if let Some(tx) = sender {
        let _ = tx.send(approved);
        let text = if approved { "Onaylandı" } else { "Reddedildi" };
        answer_callback_query(client, bot_token, &callback.id, text).await;
    } else {
        answer_callback_query(client, bot_token, &callback.id, "Süre doldu").await;
    }

    // Remove keyboard from the original message
    if let Some(msg) = callback.message {
        remove_keyboard(client, bot_token, chat_id, msg.message_id).await;
    }
}

async fn handle_topic_message(
    prompt: &str,
    thread_id: i64,
    config: &Config,
    sessions: &SessionMap,
    text_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> anyhow::Result<claude::ClaudeResult> {
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
                state: TopicState::Active,
                pending_approval: None,
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
        tracing::info!(thread_id, session_id = %sid, "resuming Claude session");
        claude::run_resume_streaming(config, prompt, sid, &workdir, 600, false, text_tx).await
    } else {
        tracing::info!(thread_id, "starting new Claude session");
        claude::run_task_streaming(config, prompt, None, None, &workdir, 600, text_tx).await
    };

    match claude_result {
        Ok(result) => {
            // Store claude_session_id if we got one
            if let Some(ref sid) = result.claude_session_id {
                tracing::info!(thread_id, session_id = %sid, "captured Claude session_id");
                let mut map = sessions.lock().await;
                if let Some(session) = map.get_mut(&thread_id) {
                    if session.claude_session_id.is_none() {
                        session.claude_session_id = result.claude_session_id.clone();
                    }
                }
            } else {
                tracing::warn!(thread_id, "Claude returned no session_id");
            }
            Ok(result)
        }
        Err(e) => {
            let err_msg = e.body.error;
            anyhow::bail!("{err_msg}")
        }
    }
}

