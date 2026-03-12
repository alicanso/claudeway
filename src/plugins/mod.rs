pub mod cloudflare_tunnel;
pub mod dashboard;
pub mod slack;
pub mod swagger;
pub mod telegram;

use std::path::PathBuf;
use std::sync::Arc;
use crate::config::PluginConfig;
use crate::plugin::Plugin;

/// Build the list of active plugins based on config and CLI overrides.
/// All plugins are disabled by default — enable them in claudeway.toml or via --enable-plugin.
pub fn plugin_registry(
    plugin_config: &PluginConfig,
    disabled_plugins: &[String],
    enabled_plugins: &[String],
) -> Vec<Arc<dyn Plugin>> {
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();

    if plugin_config.is_plugin_enabled("dashboard", disabled_plugins, enabled_plugins) {
        plugins.push(Arc::new(dashboard::DashboardPlugin::new()));
    }

    if plugin_config.is_plugin_enabled("swagger", disabled_plugins, enabled_plugins) {
        plugins.push(Arc::new(swagger::SwaggerPlugin));
    }

    if plugin_config.is_plugin_enabled("telegram", disabled_plugins, enabled_plugins) {
        let bot_token = plugin_config.get_str("telegram", "bot_token").unwrap_or_default();
        let chat_id = plugin_config.get_str("telegram", "chat_id").unwrap_or_default();
        let projects_dir = plugin_config
            .get_str("telegram", "projects_dir")
            .map(|s| {
                if s.starts_with("~/") {
                    dirs_next::home_dir()
                        .unwrap_or_default()
                        .join(&s[2..])
                } else {
                    PathBuf::from(s)
                }
            })
            .unwrap_or_else(|| {
                dirs_next::home_dir()
                    .unwrap_or_default()
                    .join("Documents/GitHub")
            });
        if !bot_token.is_empty() && !chat_id.is_empty() {
            plugins.push(Arc::new(telegram::TelegramPlugin::new(bot_token, chat_id, projects_dir)));
        } else {
            tracing::warn!("telegram plugin enabled but bot_token or chat_id missing");
        }
    }

    if plugin_config.is_plugin_enabled("cloudflare_tunnel", disabled_plugins, enabled_plugins) {
        let tunnel_token = plugin_config.get_str("cloudflare_tunnel", "tunnel_token");
        plugins.push(Arc::new(cloudflare_tunnel::CloudflareTunnelPlugin::new(tunnel_token)));
    }

    if plugin_config.is_plugin_enabled("slack", disabled_plugins, enabled_plugins) {
        let webhook_url = plugin_config.get_str("slack", "webhook_url").unwrap_or_default();
        if !webhook_url.is_empty() {
            plugins.push(Arc::new(slack::SlackPlugin::new(webhook_url)));
        } else {
            tracing::warn!("slack plugin enabled but webhook_url missing");
        }
    }

    plugins
}
