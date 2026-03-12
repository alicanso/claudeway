use axum::extract::Extension;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use utoipa::OpenApi;

mod auth;
mod claude;
mod config;
mod error;
mod handlers;
mod logging;
mod models;
mod session;
mod plugin;
mod plugins;
mod startup;

use config::Config;
use handlers::models::ModelsCache;
use logging::KeyLogger;
use session::SessionStore;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Claudeway",
        description = "Blazing-fast HTTP gateway for the Claude CLI. Built with Rust, Axum, and Tokio.",
        version = "0.1.0",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    paths(
        handlers::health::health,
        handlers::models::list_models,
        handlers::task::create_task,
        handlers::session::start_session,
        handlers::session::continue_session,
        handlers::session::get_session,
        handlers::session::delete_session,
    ),
    components(schemas(
        models::TokenUsage,
        models::HealthResponse,
        models::ModelsResponse,
        models::ModelInfo,
        models::TaskRequest,
        models::TaskResponse,
        models::SessionStartRequest,
        models::SessionStartResponse,
        models::SessionContinueRequest,
        models::SessionInfoResponse,
        models::DeleteSessionResponse,
        error::ApiError,
    )),
    tags(
        (name = "System", description = "Health and status endpoints"),
        (name = "Models", description = "Available Claude models"),
        (name = "Tasks", description = "One-shot Claude task execution"),
        (name = "Sessions", description = "Persistent stateful Claude sessions")
    ),
    security(("bearer" = []))
)]
pub(crate) struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = Config::load()?;

    // Interactive plugin setup on first run (no config file, no --force)
    if let Some(selected) = startup::interactive_setup(&config) {
        // Merge interactively-selected plugins into enabled_plugins
        for name in selected {
            if !config.enabled_plugins.contains(&name) {
                config.enabled_plugins.push(name);
            }
        }
    }

    let config = Arc::new(config);
    let start_time = Instant::now();
    let request_counter = Arc::new(AtomicU64::new(0));
    let store = Arc::new(SessionStore::new());
    let models_cache = Arc::new(ModelsCache::new());
    let key_ids: Vec<String> = config.key_ids().into_iter().cloned().collect();
    let logger = Arc::new(KeyLogger::new(std::path::Path::new(&config.log_dir), &key_ids)?);

    // Ensure base workdir exists
    tokio::fs::create_dir_all(&config.claude_workdir).await?;

    // Load plugin config
    let plugin_config = config::PluginConfig::load(config.config_path.as_deref())?;

    // Build plugin registry
    let plugin_list = plugins::plugin_registry(&plugin_config, &config.disabled_plugins, &config.enabled_plugins);

    // Register plugins
    let mut event_bus = plugin::EventBus::new();
    let mut plugin_routes = Router::new();
    for p in &plugin_list {
        let mut registrar = plugin::PluginRegistrar::new();
        p.on_register(&mut registrar)?;
        let (router, subscriptions) = registrar.build();
        if let Some(r) = router {
            plugin_routes = plugin_routes.merge(r);
        }
        event_bus.register(p.clone(), &subscriptions);
    }
    let event_bus = Arc::new(event_bus);

    // Create PluginContext
    let plugin_ctx = plugin::PluginContext::new(
        store.clone(),
        logger.clone(),
        config.clone(),
        request_counter.clone(),
        models_cache.clone(),
        event_bus,
        start_time,
    );

    let api_keys = Arc::new(config.api_keys.clone());

    // Public routes (no auth) — health
    let public_routes = Router::new()
        .route(
            "/health",
            get({
                let start = start_time;
                move || handlers::health::health(Arc::new(start))
            }),
        );

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route(
            "/models",
            get({
                let cache = models_cache.clone();
                move || handlers::models::list_models(cache.clone())
            }),
        )
        .route("/task", post(handlers::task::create_task))
        .route("/session/start", post(handlers::session::start_session))
        .route(
            "/session/{id}",
            post(handlers::session::continue_session)
                .get(handlers::session::get_session)
                .delete(handlers::session::delete_session),
        )
        .layer(middleware::from_fn(move |req, next| {
            let keys = api_keys.clone();
            auth::auth_middleware(req, next, keys)
        }))
        .layer(Extension(request_counter.clone()))
        .layer(Extension(config.clone()))
        .layer(Extension(store.clone()))
        .layer(Extension(logger.clone()));

    // Dashboard handlers still extract individual Extension<Arc<...>> types.
    // Layer them on plugin_routes so they're available.
    let plugin_routes = plugin_routes
        .layer(Extension(config.clone()))
        .layer(Extension(Arc::new(start_time)))
        .layer(Extension(request_counter.clone()))
        .layer(Extension(store.clone()))
        .layer(Extension(logger.clone()));

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(plugin_routes)
        .layer(Extension(plugin_ctx.clone()));

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr).await?;

    let enabled_plugin_names: Vec<String> = plugin_list.iter().map(|p| p.name().to_string()).collect();
    startup::print_banner(&config, &enabled_plugin_names);

    plugin_ctx.emit(plugin::GatewayEvent::ServerStarted { port: config.port });

    axum::serve(listener, app).await?;
    Ok(())
}
