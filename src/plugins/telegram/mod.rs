pub mod markdown;
pub mod polling;
pub mod repos;

use crate::plugin::{EventType, GatewayEvent, Plugin, PluginContext, PluginRegistrar};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Telegram bot plugin — two-way Claude chat via Forum Topics + event notifications.
///
/// Each Forum Topic maps to an independent Claude session.
/// Also sends notifications for RequestCompleted and SessionCompleted events.
///
/// ```toml
/// [plugins.telegram]
/// enabled = true
/// bot_token = "123456:ABC-DEF..."
/// chat_id = "-1001234567890"
/// ```
pub struct TelegramPlugin {
    bot_token: String,
    chat_id: String,
    projects_dir: PathBuf,
    sessions: polling::SessionMap,
    polling_handle: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl TelegramPlugin {
    pub fn new(bot_token: String, chat_id: String, projects_dir: PathBuf) -> Self {
        Self {
            bot_token,
            chat_id,
            projects_dir,
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            polling_handle: tokio::sync::Mutex::new(None),
        }
    }
}

impl Plugin for TelegramPlugin {
    fn name(&self) -> &str {
        "telegram"
    }

    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        registrar.subscribe(EventType::ServerStarted);
        registrar.subscribe(EventType::RequestCompleted);
        registrar.subscribe(EventType::SessionCompleted);
        Ok(())
    }

    fn on_event(
        &self,
        event: &GatewayEvent,
        ctx: &PluginContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        match event {
            GatewayEvent::ServerStarted { .. } => {
                let config = ctx.config.clone();
                let bot_token = self.bot_token.clone();
                let chat_id = self.chat_id.clone();
                let sessions = self.sessions.clone();
                let projects_dir = self.projects_dir.clone();

                Box::pin(async move {
                    let handle = tokio::spawn(polling::run_polling_loop(
                        bot_token, chat_id, config, sessions, projects_dir,
                    ));
                    let mut guard = self.polling_handle.lock().await;
                    *guard = Some(handle);
                    tracing::info!("telegram polling loop started");
                    Ok(())
                })
            }

            GatewayEvent::RequestCompleted {
                key_id,
                path,
                status,
                duration,
            } => {
                let text = format!(
                    "Request completed: {path} (key: {key_id}, status: {status}, {duration:?})"
                );
                let bot_token = self.bot_token.clone();
                let chat_id = self.chat_id.clone();
                Box::pin(async move {
                    let client = reqwest::Client::new();
                    let _ =
                        polling::send_message(&client, &bot_token, &chat_id, None, &text).await;
                    Ok(())
                })
            }

            GatewayEvent::SessionCompleted {
                session_id,
                token_usage,
            } => {
                let text = format!(
                    "Session completed: {session_id} (tokens: {} in / {} out)",
                    token_usage.input, token_usage.output
                );
                let bot_token = self.bot_token.clone();
                let chat_id = self.chat_id.clone();
                Box::pin(async move {
                    let client = reqwest::Client::new();
                    let _ =
                        polling::send_message(&client, &bot_token, &chat_id, None, &text).await;
                    Ok(())
                })
            }

            _ => Box::pin(async { Ok(()) }),
        }
    }

    fn on_shutdown(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async {
            let mut guard = self.polling_handle.lock().await;
            if let Some(handle) = guard.take() {
                tracing::info!("stopping telegram polling loop...");
                handle.abort();
                tracing::info!("telegram polling loop stopped");
            }
            Ok(())
        })
    }
}
