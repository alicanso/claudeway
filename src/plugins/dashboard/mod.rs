use crate::plugin::{Plugin, PluginRegistrar};

pub struct DashboardPlugin;

impl DashboardPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for DashboardPlugin {
    fn name(&self) -> &str {
        "dashboard"
    }

    fn on_register(&self, _registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        // TODO: will be filled in during migration task
        Ok(())
    }
}
