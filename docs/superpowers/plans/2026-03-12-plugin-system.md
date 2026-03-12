# Plugin System Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform Claudeway into a plugin-based architecture with a `Plugin` trait, `PluginContext`, `EventBus`, config file support, and migrate existing dashboard/swagger features into plugins.

**Architecture:** A `Plugin` trait defines the plugin interface. Plugins register HTTP routes via `PluginRegistrar` and subscribe to `GatewayEvent`s via `EventBus`. State access is controlled through `PluginContext`. Config is loaded from `claudeway.toml` with CLI overrides.

**Tech Stack:** Rust 2024 edition, Axum 0.8, Tokio, toml, serde. Native async trait syntax (no `async-trait` crate — Rust 2024 supports `async fn` in traits natively). Uses `trait_variant` for `Send`-bound object-safe async traits.

**Spec:** `docs/superpowers/specs/2026-03-12-plugin-system-design.md`

---

## File Structure

```
src/
├── plugin.rs              # NEW: Plugin trait, PluginRegistrar, PluginContext, EventType, GatewayEvent, EventBus
├── plugins/
│   ├── mod.rs             # NEW: plugin_registry(), is_plugin_enabled()
│   ├── dashboard/
│   │   ├── mod.rs         # NEW: DashboardPlugin struct + Plugin impl
│   │   ├── auth.rs        # MOVE: src/admin_auth.rs → unchanged content
│   │   ├── models.rs      # MOVE: src/admin_models.rs → unchanged content
│   │   ├── stats.rs       # MOVE: src/admin_stats.rs → update import paths
│   │   ├── handlers.rs    # MOVE: src/handlers/admin.rs → update import paths
│   │   └── assets.rs      # MOVE: src/dashboard.rs → unchanged content
│   └── swagger/
│       └── mod.rs         # NEW: SwaggerPlugin struct + Plugin impl
├── config.rs              # MODIFY: add PluginConfig, --config, --disable-plugin
├── main.rs                # MODIFY: use plugin_registry(), wire EventBus
├── handlers/
│   ├── mod.rs             # MODIFY: remove admin module
│   ├── task.rs            # MODIFY: emit events via PluginContext
│   └── session.rs         # MODIFY: emit events via PluginContext
└── Cargo.toml             # MODIFY: add toml, trait-variant dependencies
```

---

## Chunk 1: Core Plugin Infrastructure

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml:11-32`

- [ ] **Step 1: Add toml and trait-variant to Cargo.toml**

Add after `mime_guess` (line 32), before `[features]`:

```toml
toml_crate = { package = "toml", version = "0.8" }
trait-variant = "0.1"
```

We use `toml_crate` as the key with `package = "toml"` to avoid collision with Cargo's built-in `toml` namespace. `trait-variant` provides the `#[trait_variant::make(SendPlugin: Send)]` macro for object-safe async traits with `Send` bounds.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: OK (no code uses these yet)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add toml and trait-variant dependencies"
```

---

### Task 2: Create plugin.rs — Plugin trait, EventBus, PluginContext

**Files:**
- Create: `src/plugin.rs`

- [ ] **Step 1: Write the complete plugin.rs**

```rust
// src/plugin.rs
use axum::Router;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::handlers::models::ModelsCache;
use crate::logging::KeyLogger;
use crate::models::TokenUsage;
use crate::session::SessionStore;

// ── Plugin Trait ──
// Uses trait_variant to generate a SendPlugin variant with Send bounds on async methods.
// This allows Arc<dyn SendPlugin> to be used with tokio::spawn.

#[trait_variant::make(SendPlugin: Send)]
pub trait Plugin: Sync + 'static {
    fn name(&self) -> &str;
    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()>;
    async fn on_event(&self, _event: &GatewayEvent, _ctx: &PluginContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn on_shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

// ── PluginRegistrar ──

pub struct PluginRegistrar {
    router: Option<Router>,
    subscriptions: Vec<EventType>,
}

impl PluginRegistrar {
    pub fn new() -> Self {
        Self {
            router: None,
            subscriptions: Vec::new(),
        }
    }

    pub fn add_routes(&mut self, router: Router) {
        self.router = match self.router.take() {
            Some(existing) => Some(existing.merge(router)),
            None => Some(router),
        };
    }

    pub fn subscribe(&mut self, event: EventType) {
        self.subscriptions.push(event);
    }

    pub fn build(self) -> (Option<Router>, Vec<EventType>) {
        (self.router, self.subscriptions)
    }
}

// ── EventType ──

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum EventType {
    ServerStarted,
    ServerShutdown,
    RequestReceived,
    RequestCompleted,
    SessionStarted,
    SessionCompleted,
    SessionDeleted,
    CostRecorded,
}

// ── GatewayEvent ──

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    ServerStarted { port: u16 },
    ServerShutdown,
    RequestReceived { key_id: String, path: String, method: String },
    RequestCompleted { key_id: String, path: String, status: u16, duration: Duration },
    SessionStarted { session_id: String, model: String, key_id: String },
    SessionCompleted { session_id: String, token_usage: TokenUsage },
    SessionDeleted { session_id: String },
    CostRecorded { key_id: String, model: String, cost: f64 },
}

impl GatewayEvent {
    pub fn event_type(&self) -> EventType {
        match self {
            Self::ServerStarted { .. } => EventType::ServerStarted,
            Self::ServerShutdown => EventType::ServerShutdown,
            Self::RequestReceived { .. } => EventType::RequestReceived,
            Self::RequestCompleted { .. } => EventType::RequestCompleted,
            Self::SessionStarted { .. } => EventType::SessionStarted,
            Self::SessionCompleted { .. } => EventType::SessionCompleted,
            Self::SessionDeleted { .. } => EventType::SessionDeleted,
            Self::CostRecorded { .. } => EventType::CostRecorded,
        }
    }
}

// ── EventBus ──
// Registration happens only at startup (before server accepts connections),
// so no concurrent mutation concerns. emit() is called from handlers concurrently
// but only reads the subscriber map.

pub struct EventBus {
    subscribers: HashMap<EventType, Vec<(String, Arc<dyn SendPlugin>)>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn SendPlugin>, subscriptions: &[EventType]) {
        for event_type in subscriptions {
            self.subscribers
                .entry(*event_type)
                .or_default()
                .push((plugin.name().to_string(), plugin.clone()));
        }
    }

    pub fn emit(&self, event: GatewayEvent, ctx: PluginContext) {
        let event_type = event.event_type();
        if let Some(subs) = self.subscribers.get(&event_type) {
            let event = Arc::new(event);
            for (plugin_name, plugin) in subs {
                let event = event.clone();
                let ctx = ctx.clone();
                let plugin = plugin.clone();
                let name = plugin_name.clone();
                tokio::spawn(async move {
                    if let Err(e) = plugin.on_event(&event, &ctx).await {
                        tracing::error!(plugin = %name, error = %e, "plugin event handler failed");
                    }
                });
            }
        }
    }
}

// ── PluginContext ──

#[derive(Clone)]
pub struct PluginContext {
    pub(crate) session_store: Arc<SessionStore>,
    pub(crate) key_logger: Arc<KeyLogger>,
    pub(crate) config: Arc<Config>,
    pub(crate) request_counter: Arc<AtomicU64>,
    pub(crate) models_cache: Arc<ModelsCache>,
    pub(crate) event_bus: Arc<EventBus>,
    pub(crate) start_time: Instant,
}

impl PluginContext {
    pub fn new(
        session_store: Arc<SessionStore>,
        key_logger: Arc<KeyLogger>,
        config: Arc<Config>,
        request_counter: Arc<AtomicU64>,
        models_cache: Arc<ModelsCache>,
        event_bus: Arc<EventBus>,
        start_time: Instant,
    ) -> Self {
        Self {
            session_store,
            key_logger,
            config,
            request_counter,
            models_cache,
            event_bus,
            start_time,
        }
    }

    pub fn active_session_count(&self) -> usize {
        self.session_store.list_all().len()
    }

    pub fn total_request_count(&self) -> u64 {
        self.request_counter.load(Ordering::Relaxed)
    }

    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn session_store(&self) -> &SessionStore {
        &self.session_store
    }

    pub fn key_logger(&self) -> &KeyLogger {
        &self.key_logger
    }

    pub fn models_cache(&self) -> &ModelsCache {
        &self.models_cache
    }

    pub fn emit(&self, event: GatewayEvent) {
        self.event_bus.emit(event, self.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_mapping() {
        let event = GatewayEvent::ServerStarted { port: 3000 };
        assert_eq!(event.event_type(), EventType::ServerStarted);

        let event = GatewayEvent::CostRecorded {
            key_id: "k".into(),
            model: "m".into(),
            cost: 0.01,
        };
        assert_eq!(event.event_type(), EventType::CostRecorded);
    }

    #[test]
    fn test_registrar_build_empty() {
        let registrar = PluginRegistrar::new();
        let (router, subs) = registrar.build();
        assert!(router.is_none());
        assert!(subs.is_empty());
    }

    #[test]
    fn test_registrar_subscribe() {
        let mut registrar = PluginRegistrar::new();
        registrar.subscribe(EventType::ServerStarted);
        registrar.subscribe(EventType::CostRecorded);
        let (_, subs) = registrar.build();
        assert_eq!(subs.len(), 2);
    }

    #[test]
    fn test_event_bus_no_subscribers() {
        // emit with no subscribers should not panic
        let bus = EventBus::new();
        // No PluginContext needed since there are no subscribers
        let event = GatewayEvent::ServerShutdown;
        assert_eq!(event.event_type(), EventType::ServerShutdown);
    }
}
```

- [ ] **Step 2: Add `mod plugin;` to main.rs**

Add `mod plugin;` after `mod session;` (line 21) in `src/main.rs`. Don't use the module yet — just declare it.

```rust
mod session;
mod plugin;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: PASS (with unused warnings, which is fine)

- [ ] **Step 4: Run tests**

Run: `cargo test plugin::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/plugin.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: add core plugin infrastructure (Plugin trait, EventBus, PluginContext)"
```

---

## Chunk 2: Config System and Plugin Registry

### Task 3: Add PluginConfig and config file loading

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add PathBuf import to config.rs**

Add at the top of `src/config.rs`, after the existing imports (line 3):

```rust
use std::path::{Path, PathBuf};
```

- [ ] **Step 2: Add new CLI fields to the Cli struct**

Add after `log_level` field (line 31), before the closing brace of `Cli`:

```rust
    /// Path to config file (claudeway.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Disable specific plugins (overrides config file)
    #[arg(long, value_delimiter = ',')]
    disable_plugin: Vec<String>,
```

- [ ] **Step 3: Add new fields to Config struct**

Add to the `Config` struct (after `generated_key` field, line 43):

```rust
    pub config_path: Option<PathBuf>,
    pub disabled_plugins: Vec<String>,
```

- [ ] **Step 4: Populate new fields in Config::load()**

In `Config::load()`, add these to the `Ok(Self { ... })` block (after `generated_key`, line 71):

```rust
            config_path: cli.config,
            disabled_plugins: cli.disable_plugin,
```

- [ ] **Step 5: Add PluginConfig struct and loading function**

Add after `generate_secret` function (after line 158), before `#[cfg(test)]`:

```rust
/// Config loaded from claudeway.toml file
#[derive(serde::Deserialize, Default)]
pub struct PluginConfig {
    #[serde(default)]
    pub plugins: HashMap<String, toml_crate::Value>,
}

impl PluginConfig {
    /// Load from file path, or return default if no config file found
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let config_path = match path {
            Some(p) => {
                if p.exists() {
                    Some(p.to_path_buf())
                } else {
                    return Err(anyhow::anyhow!("Config file not found: {}", p.display()));
                }
            }
            None => {
                let default_path = PathBuf::from("claudeway.toml");
                if default_path.exists() {
                    Some(default_path)
                } else {
                    None
                }
            }
        };

        match config_path {
            Some(path) => {
                let content = std::fs::read_to_string(&path)?;
                let config: PluginConfig = toml_crate::from_str(&content)?;
                Ok(config)
            }
            None => Ok(PluginConfig::default()),
        }
    }

    pub fn is_plugin_enabled(&self, name: &str, disabled_plugins: &[String]) -> bool {
        if disabled_plugins.iter().any(|d| d == name) {
            return false;
        }
        self.plugins
            .get(name)
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true) // enabled by default if compiled in
    }
}
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check`
Expected: PASS

- [ ] **Step 7: Write tests for PluginConfig**

Add to the `#[cfg(test)]` block in `config.rs`:

```rust
    #[test]
    fn test_plugin_config_default_enabled() {
        let config = PluginConfig::default();
        assert!(config.is_plugin_enabled("dashboard", &[]));
    }

    #[test]
    fn test_plugin_config_disabled_by_cli() {
        let config = PluginConfig::default();
        let disabled = vec!["dashboard".to_string()];
        assert!(!config.is_plugin_enabled("dashboard", &disabled));
    }

    #[test]
    fn test_plugin_config_disabled_in_file() {
        let toml_str = r#"
            [plugins.dashboard]
            enabled = false
        "#;
        let config: PluginConfig = toml_crate::from_str(toml_str).unwrap();
        assert!(!config.is_plugin_enabled("dashboard", &[]));
    }

    #[test]
    fn test_plugin_config_enabled_in_file() {
        let toml_str = r#"
            [plugins.swagger]
            enabled = true
        "#;
        let config: PluginConfig = toml_crate::from_str(toml_str).unwrap();
        assert!(config.is_plugin_enabled("swagger", &[]));
    }
```

- [ ] **Step 8: Run tests**

Run: `cargo test config::tests`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add src/config.rs
git commit -m "feat: add PluginConfig with toml file loading and CLI overrides"
```

---

### Task 4: Create plugins/mod.rs with plugin_registry()

**Files:**
- Create: `src/plugins/mod.rs`
- Create: `src/plugins/dashboard/mod.rs` (stub)
- Create: `src/plugins/swagger/mod.rs` (stub)

- [ ] **Step 1: Create the plugins module with registry function**

```rust
// src/plugins/mod.rs

#[cfg(feature = "dashboard")]
pub mod dashboard;
#[cfg(feature = "swagger")]
pub mod swagger;

use std::sync::Arc;
use crate::config::PluginConfig;
use crate::plugin::SendPlugin;

/// Build the list of active plugins based on config and feature flags.
pub fn plugin_registry(
    plugin_config: &PluginConfig,
    disabled_plugins: &[String],
) -> Vec<Arc<dyn SendPlugin>> {
    let mut plugins: Vec<Arc<dyn SendPlugin>> = Vec::new();

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
```

- [ ] **Step 2: Create placeholder dashboard module**

Create `src/plugins/dashboard/mod.rs`:

```rust
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
```

- [ ] **Step 3: Create placeholder swagger module**

Create `src/plugins/swagger/mod.rs`:

```rust
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
```

- [ ] **Step 4: Add `mod plugins;` to main.rs**

Add after `mod plugin;`:

```rust
mod plugins;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --all-features`
Expected: PASS (with unused warnings)

- [ ] **Step 6: Commit**

```bash
git add src/plugins/ src/main.rs
git commit -m "feat: add plugin registry with dashboard and swagger stubs"
```

---

## Chunk 3: Migrate Swagger Plugin

### Task 5: Implement SwaggerPlugin with route registration

**Files:**
- Modify: `src/plugins/swagger/mod.rs`

- [ ] **Step 1: Implement SwaggerPlugin on_register**

Update `src/plugins/swagger/mod.rs`:

```rust
// src/plugins/swagger/mod.rs

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
```

- [ ] **Step 2: Make ApiDoc accessible from plugins**

In `src/main.rs`, change `struct ApiDoc;` (line 75) to `pub(crate) struct ApiDoc;`.

- [ ] **Step 3: Verify swagger plugin compiles**

Run: `cargo check --features swagger`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/plugins/swagger/mod.rs src/main.rs
git commit -m "feat: implement SwaggerPlugin with route registration"
```

---

## Chunk 4: Migrate Dashboard Plugin

### Task 6: Move dashboard files into plugins/dashboard/

**Files:**
- Move: `src/admin_auth.rs` → `src/plugins/dashboard/auth.rs`
- Move: `src/admin_models.rs` → `src/plugins/dashboard/models.rs`
- Move: `src/admin_stats.rs` → `src/plugins/dashboard/stats.rs`
- Move: `src/handlers/admin.rs` → `src/plugins/dashboard/handlers.rs`
- Move: `src/dashboard.rs` → `src/plugins/dashboard/assets.rs`

- [ ] **Step 1: Copy the files**

```bash
cp src/admin_auth.rs src/plugins/dashboard/auth.rs
cp src/admin_models.rs src/plugins/dashboard/models.rs
cp src/admin_stats.rs src/plugins/dashboard/stats.rs
cp src/handlers/admin.rs src/plugins/dashboard/handlers.rs
cp src/dashboard.rs src/plugins/dashboard/assets.rs
```

- [ ] **Step 2: Update ALL import paths in stats.rs**

In `src/plugins/dashboard/stats.rs`:

Change line 4:
```rust
use crate::admin_models::{CostEntry, KeyCost, ModelBreakdown};
```
to:
```rust
use super::models::{CostEntry, KeyCost, ModelBreakdown};
```

Also update the inline references on lines 123 and 137:
```rust
// Line 123 — change:
pub fn get_keys_stats(log_dir: &Path, key_ids: &[String]) -> Vec<crate::admin_models::KeyStats> {
// to:
pub fn get_keys_stats(log_dir: &Path, key_ids: &[String]) -> Vec<super::models::KeyStats> {

// Line 137 — change:
    .map(|(key_id, (reqs, cost))| crate::admin_models::KeyStats {
// to:
    .map(|(key_id, (reqs, cost))| super::models::KeyStats {
```

- [ ] **Step 3: Update ALL import paths in handlers.rs**

In `src/plugins/dashboard/handlers.rs`:

Change line 10-11:
```rust
use crate::admin_auth::AdminSessionStore;
use crate::admin_models::*;
```
to:
```rust
use super::auth::AdminSessionStore;
use super::models::*;
```

Change line 68 (inside `overview` handler):
```rust
    let models_breakdown = crate::admin_stats::get_models_breakdown(logger.log_dir());
```
to:
```rust
    let models_breakdown = super::stats::get_models_breakdown(logger.log_dir());
```

Change line 181 (inside `get_keys` handler):
```rust
    let keys = crate::admin_stats::get_keys_stats(log_dir, &key_ids);
```
to:
```rust
    let keys = super::stats::get_keys_stats(log_dir, &key_ids);
```

Change line 195 (inside `get_costs` handler):
```rust
    let data = crate::admin_stats::aggregate_costs(log_dir, group_by);
```
to:
```rust
    let data = super::stats::aggregate_costs(log_dir, group_by);
```

- [ ] **Step 4: Update mod.rs to declare submodules and implement DashboardPlugin**

Update `src/plugins/dashboard/mod.rs`:

```rust
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
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check --features dashboard`
Expected: PASS (old modules still present, both compile)

- [ ] **Step 6: Commit**

```bash
git add src/plugins/dashboard/
git commit -m "feat: migrate dashboard files into plugins/dashboard module"
```

---

### Task 7: Remove old dashboard files and update main.rs references

**Files:**
- Delete: `src/admin_auth.rs`, `src/admin_models.rs`, `src/admin_stats.rs`, `src/dashboard.rs`, `src/handlers/admin.rs`
- Modify: `src/main.rs` (remove old mod declarations and inline route setup)
- Modify: `src/handlers/mod.rs` (remove admin module)

- [ ] **Step 1: Remove old files**

```bash
rm src/admin_auth.rs src/admin_models.rs src/admin_stats.rs src/dashboard.rs src/handlers/admin.rs
```

- [ ] **Step 2: Remove old mod declarations from main.rs**

Remove these lines from `src/main.rs` (lines 22-29):

```rust
#[cfg(feature = "dashboard")]
mod admin_auth;
#[cfg(feature = "dashboard")]
mod admin_models;
#[cfg(feature = "dashboard")]
mod admin_stats;
#[cfg(feature = "dashboard")]
mod dashboard;
```

- [ ] **Step 3: Remove admin module from handlers/mod.rs**

Remove these lines from `src/handlers/mod.rs` (lines 5-6):

```rust
#[cfg(feature = "dashboard")]
pub mod admin;
```

- [ ] **Step 4: Remove inline dashboard/swagger route setup from main.rs**

Remove the `#[cfg(feature = "swagger")]` import at the top (lines 11-12):
```rust
#[cfg(feature = "swagger")]
use utoipa_swagger_ui::SwaggerUi;
```

Remove the inline swagger setup (lines 115-119):
```rust
    #[cfg(feature = "swagger")]
    {
        public_routes = public_routes
            .merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()));
    }
```

Remove the admin session store + admin routes + dashboard route merge (lines 147-179):
```rust
    #[cfg(feature = "dashboard")]
    let admin_session_store = Arc::new(admin_auth::AdminSessionStore::new());

    #[cfg(feature = "dashboard")]
    let admin_routes = { ... };

    // ... and the merge block:
    #[cfg(feature = "dashboard")]
    {
        app = app
            .merge(admin_routes)
            .route("/dashboard", ...)
            .route("/dashboard/{*rest}", ...);
    }
```

- [ ] **Step 5: Add plugin loading to main.rs**

Replace the removed code. Change `start_time` from `Arc<Instant>` to plain `Instant` (line 93):

```rust
    let start_time = Instant::now();
```

After creating `logger` (line 98), add plugin setup:

```rust
    // Load plugin config
    let plugin_config = crate::config::PluginConfig::load(
        config.config_path.as_deref()
    )?;

    // Build plugin registry
    let plugin_list = plugins::plugin_registry(&plugin_config, &config.disabled_plugins);

    // Register plugins
    let mut event_bus = crate::plugin::EventBus::new();
    let mut plugin_routes = Router::new();
    for plugin in &plugin_list {
        let mut registrar = crate::plugin::PluginRegistrar::new();
        plugin.on_register(&mut registrar)?;
        let (router, subscriptions) = registrar.build();
        if let Some(r) = router {
            plugin_routes = plugin_routes.merge(r);
        }
        event_bus.register(plugin.clone(), &subscriptions);
    }
    let event_bus = Arc::new(event_bus);

    // Create PluginContext
    let plugin_ctx = crate::plugin::PluginContext::new(
        store.clone(),
        logger.clone(),
        config.clone(),
        request_counter.clone(),
        models_cache.clone(),
        event_bus,
        start_time,
    );
```

Update the health handler to use `start_time` directly (since it's now `Instant`, not `Arc<Instant>`):

```rust
    let mut public_routes = Router::new()
        .route(
            "/health",
            get({
                let start = start_time;
                move || handlers::health::health(Arc::new(start))
            }),
        );
```

Layer Extensions onto plugin_routes for dashboard handler compatibility, then merge:

```rust
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
```

- [ ] **Step 6: Verify it compiles with all features**

Run: `cargo check --all-features`
Expected: PASS

- [ ] **Step 7: Verify it compiles without features**

Run: `cargo check`
Expected: PASS

- [ ] **Step 8: Run existing tests**

Run: `cargo test --all-features`
Expected: All existing tests PASS

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: wire plugin system into main.rs, remove old inline feature setup"
```

---

## Chunk 5: Event Emission from Core Handlers

### Task 8: Emit events from task and session handlers

**Files:**
- Modify: `src/handlers/task.rs`
- Modify: `src/handlers/session.rs`

- [ ] **Step 1: Read current task.rs and session.rs**

Read the files to understand the current handler signatures and identify where to add event emission. The handlers need to extract `Extension(plugin_ctx): Extension<PluginContext>` and call `plugin_ctx.emit(...)` at appropriate points.

- [ ] **Step 2: Add PluginContext Extension to task handler**

In `src/handlers/task.rs`, add the import and PluginContext extraction:

```rust
use crate::plugin::{PluginContext, GatewayEvent};
```

Add `Extension(plugin_ctx): Extension<PluginContext>` to the `create_task` handler signature. After the task completes and cost is calculated, emit:

```rust
plugin_ctx.emit(GatewayEvent::CostRecorded {
    key_id: key_id.clone(),
    model: model.clone(),
    cost,
});
```

- [ ] **Step 3: Add PluginContext Extension to session handlers**

In `src/handlers/session.rs`, add the import:

```rust
use crate::plugin::{PluginContext, GatewayEvent};
```

Add `Extension(plugin_ctx): Extension<PluginContext>` to each handler signature and emit:
- `start_session`: emit `SessionStarted { session_id, model, key_id }` after creating a session
- `continue_session`: emit `SessionCompleted { session_id, token_usage }` + `CostRecorded { key_id, model, cost }` after each turn
- `delete_session`: emit `SessionDeleted { session_id }` after removing a session

- [ ] **Step 4: Verify it compiles**

Run: `cargo check --all-features`
Expected: PASS

- [ ] **Step 5: Run tests**

Run: `cargo test --all-features`
Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/handlers/task.rs src/handlers/session.rs
git commit -m "feat: emit gateway events from task and session handlers"
```

---

### Task 9: Emit ServerStarted event after listener binds

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Emit ServerStarted after the listener binds**

In `src/main.rs`, after `TcpListener::bind(addr).await?` and the startup log messages, add:

```rust
    plugin_ctx.emit(crate::plugin::GatewayEvent::ServerStarted { port: config.port });
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --all-features`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: emit ServerStarted event after listener binds"
```

---

## Chunk 6: Final Verification

### Task 10: End-to-end verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test --all-features`
Expected: All tests PASS

- [ ] **Step 2: Build all feature combinations**

```bash
cargo build
cargo build --features swagger
cargo build --features dashboard
cargo build --all-features
```

Expected: All builds succeed

- [ ] **Step 3: Verify no clippy warnings**

Run: `cargo clippy --all-features -- -D warnings`
Expected: PASS (or fix any warnings)

- [ ] **Step 4: Test with a config file**

Create a test `claudeway.toml`:

```toml
[plugins.dashboard]
enabled = true

[plugins.swagger]
enabled = false
```

Run: `cargo run --all-features -- --config claudeway.toml --keys test:sk-test`
Expected: Starts up, dashboard routes work, swagger routes should not be available.

- [ ] **Step 5: Test with --disable-plugin**

Run: `cargo run --all-features -- --disable-plugin dashboard --keys test:sk-test`
Expected: Starts up, dashboard routes are not available.

- [ ] **Step 6: Final commit (cleanup if needed)**

```bash
git add -A
git commit -m "chore: plugin system cleanup and verification"
```
