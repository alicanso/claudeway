pub mod auth;
pub mod models;
pub mod stats;
pub mod handlers;
pub mod assets;

use std::sync::Arc;
use axum::extract::Extension;
use axum::routing::{get, post};
use axum::Router;

use crate::plugin::{Plugin, PluginRegistrar, EventType};
use self::auth::AdminSessionStore;

pub struct DashboardPlugin {
    admin_store: Arc<AdminSessionStore>,
}

impl DashboardPlugin {
    pub fn new() -> Self {
        Self {
            admin_store: Arc::new(AdminSessionStore::new()),
        }
    }
}

impl Plugin for DashboardPlugin {
    fn name(&self) -> &str {
        "dashboard"
    }

    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        let store = self.admin_store.clone();
        registrar.add_routes(
            Router::new()
                .route("/admin/login", post(handlers::login))
                .route("/admin/overview", get(handlers::overview))
                .route("/admin/sessions", get(handlers::list_sessions))
                .route("/admin/sessions/{id}", get(handlers::get_session_detail))
                .route("/admin/logs", get(handlers::get_logs))
                .route("/admin/keys", get(handlers::get_keys))
                .route("/admin/costs", get(handlers::get_costs))
                .layer(Extension(store))
        );
        registrar.add_routes(
            Router::new()
                .route("/dashboard", get(assets::serve_dashboard))
                .route("/dashboard/{*rest}", get(assets::serve_dashboard))
        );
        registrar.subscribe(EventType::ServerStarted);
        Ok(())
    }
}
