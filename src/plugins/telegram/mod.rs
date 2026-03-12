use crate::plugin::{EventType, GatewayEvent, Plugin, PluginContext, PluginRegistrar};
use std::future::Future;
use std::pin::Pin;

/// Telegram bot adapter — forwards Claude responses to a Telegram chat.
/// Configure via claudeway.toml:
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
}

impl TelegramPlugin {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self { bot_token, chat_id }
    }
}

impl Plugin for TelegramPlugin {
    fn name(&self) -> &str {
        "telegram"
    }

    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        registrar.subscribe(EventType::RequestCompleted);
        registrar.subscribe(EventType::SessionCompleted);
        Ok(())
    }

    fn on_event(
        &self,
        event: &GatewayEvent,
        _ctx: &PluginContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        let message = match event {
            GatewayEvent::RequestCompleted { key_id, path, status, duration } => {
                Some(format!(
                    "Request completed: {path} (key: {key_id}, status: {status}, {duration:?})"
                ))
            }
            GatewayEvent::SessionCompleted { session_id, token_usage } => {
                Some(format!(
                    "Session completed: {session_id} (tokens: {} in / {} out)",
                    token_usage.input, token_usage.output
                ))
            }
            _ => None,
        };

        Box::pin(async move {
            if let Some(text) = message {
                let url = format!(
                    "https://api.telegram.org/bot{}/sendMessage",
                    self.bot_token
                );
                let client = reqwest::Client::new();
                client
                    .post(&url)
                    .json(&serde_json::json!({
                        "chat_id": self.chat_id,
                        "text": text,
                        "parse_mode": "HTML"
                    }))
                    .send()
                    .await?;
            }
            Ok(())
        })
    }
}
