use crate::plugin::{Plugin, PluginRegistrar};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub struct SwaggerPlugin;

impl Plugin for SwaggerPlugin {
    fn name(&self) -> &str {
        "swagger"
    }

    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        registrar.add_routes(
            SwaggerUi::new("/docs")
                .url("/openapi.json", crate::ApiDoc::openapi())
                .into(),
        );
        Ok(())
    }
}
