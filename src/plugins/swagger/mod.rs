use crate::plugin::{Plugin, PluginRegistrar};

pub struct SwaggerPlugin;

impl Plugin for SwaggerPlugin {
    fn name(&self) -> &str {
        "swagger"
    }

    fn on_register(&self, _registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        // TODO: will be filled in during migration task
        Ok(())
    }
}
