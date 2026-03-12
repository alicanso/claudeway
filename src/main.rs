use axum::extract::Extension;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

mod auth;
mod claude;
mod config;
mod error;
mod handlers;
mod logging;
mod models;
mod session;

use config::Config;
use handlers::models::ModelsCache;
use logging::KeyLogger;
use session::SessionStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let config = Arc::new(config);
    let start_time = Arc::new(Instant::now());
    let store = Arc::new(SessionStore::new());
    let models_cache = Arc::new(ModelsCache::new());
    let key_ids: Vec<String> = config.key_ids().into_iter().cloned().collect();
    let logger = Arc::new(KeyLogger::new(std::path::Path::new(&config.log_dir), &key_ids)?);

    // Ensure base workdir exists
    tokio::fs::create_dir_all(&config.claude_workdir).await?;

    let api_keys = Arc::new(config.api_keys.clone());

    // Public routes (no auth)
    let public_routes = Router::new().route(
        "/health",
        get({
            let start_time = start_time.clone();
            move || handlers::health::health(start_time.clone())
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
        .layer(Extension(config.clone()))
        .layer(Extension(store))
        .layer(Extension(logger));

    let app = Router::new().merge(public_routes).merge(protected_routes);

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
