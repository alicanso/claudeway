#[cfg(feature = "dashboard")]
pub mod dashboard;
#[cfg(feature = "swagger")]
pub mod swagger;

use std::sync::Arc;
use crate::config::PluginConfig;
use crate::plugin::Plugin;

/// Build the list of active plugins based on config and feature flags.
pub fn plugin_registry(
    plugin_config: &PluginConfig,
    disabled_plugins: &[String],
) -> Vec<Arc<dyn Plugin>> {
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();

    #[cfg(feature = "dashboard")]
    {
        if plugin_config.is_plugin_enabled("dashboard", disabled_plugins) {
            plugins.push(Arc::new(dashboard::DashboardPlugin::new()));
        }
    }

    #[cfg(feature = "swagger")]
    {
        if plugin_config.is_plugin_enabled("swagger", disabled_plugins) {
            plugins.push(Arc::new(swagger::SwaggerPlugin));
        }
    }

    plugins
}
