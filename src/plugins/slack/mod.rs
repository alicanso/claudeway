use crate::plugin::{EventType, GatewayEvent, Plugin, PluginContext, PluginRegistrar};
use std::future::Future;
use std::pin::Pin;

/// Slack webhook adapter — posts Claude activity to a Slack channel.
/// Configure via claudeway.toml:
///
/// ```toml
/// [plugins.slack]
/// enabled = true
/// webhook_url = "https://hooks.slack.com/services/T.../B.../..."
/// ```
pub struct SlackPlugin {
    webhook_url: String,
}

impl SlackPlugin {
    pub fn new(webhook_url: String) -> Self {
        Self { webhook_url }
    }
}

impl Plugin for SlackPlugin {
    fn name(&self) -> &str {
        "slack"
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
                    ":white_check_mark: *Request completed*\n`{path}` — key: `{key_id}`, status: {status}, {duration:?}"
                ))
            }
            GatewayEvent::SessionCompleted { session_id, token_usage } => {
                Some(format!(
                    ":brain: *Session completed*\n`{session_id}`\nTokens: {} in / {} out",
                    token_usage.input, token_usage.output
                ))
            }
            _ => None,
        };

        Box::pin(async move {
            if let Some(text) = message {
                let client = reqwest::Client::new();
                client
                    .post(&self.webhook_url)
                    .json(&serde_json::json!({ "text": text }))
                    .send()
                    .await?;
            }
            Ok(())
        })
    }
}
