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
#[cfg(feature = "swagger")]
use utoipa_swagger_ui::SwaggerUi;

mod auth;
mod claude;
mod config;
mod error;
mod handlers;
mod logging;
mod models;
mod session;
#[cfg(feature = "dashboard")]
mod admin_auth;
#[cfg(feature = "dashboard")]
mod admin_models;
#[cfg(feature = "dashboard")]
mod admin_stats;

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
struct ApiDoc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;

    if let Some(ref key) = config.generated_key {
        eprintln!();
        eprintln!("  No API keys configured — generated one for you:");
        eprintln!();
        eprintln!("    {key}");
        eprintln!();
        eprintln!("  Use it as: curl -H \"Authorization: Bearer {key}\" http://localhost:{}/task", config.port);
        eprintln!("  To set your own keys, use --keys or WRAPPER_KEYS env var.");
        eprintln!();
    }

    let config = Arc::new(config);
    let start_time = Arc::new(Instant::now());
    let request_counter = Arc::new(AtomicU64::new(0));
    let store = Arc::new(SessionStore::new());
    let models_cache = Arc::new(ModelsCache::new());
    let key_ids: Vec<String> = config.key_ids().into_iter().cloned().collect();
    let logger = Arc::new(KeyLogger::new(std::path::Path::new(&config.log_dir), &key_ids)?);

    // Ensure base workdir exists
    tokio::fs::create_dir_all(&config.claude_workdir).await?;

    let api_keys = Arc::new(config.api_keys.clone());

    // Public routes (no auth) — health + docs
    let mut public_routes = Router::new()
        .route(
            "/health",
            get({
                let start_time = start_time.clone();
                move || handlers::health::health(start_time.clone())
            }),
        );

    #[cfg(feature = "swagger")]
    {
        public_routes = public_routes
            .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()));
    }

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
        .layer(Extension(store))
        .layer(Extension(logger));

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr).await?;
    eprintln!(
        "Claudeway v{} listening on {addr}",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("Keys loaded: {:?}", config.key_ids());

    axum::serve(listener, app).await?;
    Ok(())
}
