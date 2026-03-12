# Claudeway Plugin System Design

## Overview

Transform Claudeway from a monolithic gateway into a plugin-based architecture where features like the admin dashboard, Swagger UI, and future additions (CF Tunnel, Telegram adapter) are organized as plugins with a common trait-based interface.

## Decisions

| Decision | Choice |
|----------|--------|
| Loading model | Runtime config — all plugins compiled in via Cargo features, enabled/disabled at runtime |
| Interaction model | Hybrid — plugins can register HTTP routes AND subscribe to gateway events |
| State access | Controlled API via `PluginContext` — no direct `AppState` access |
| Configuration | `claudeway.toml` config file + CLI flag overrides |

## Architecture

### Plugin Trait

```rust
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Unique plugin name ("dashboard", "swagger", "cf-tunnel")
    fn name(&self) -> &str;

    /// Called at startup. Register routes and event subscriptions.
    /// Plugin stores its own state in Arc fields for sharing with route handlers.
    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()>;

    /// Called when a subscribed event fires. Errors are logged and swallowed.
    async fn on_event(&self, event: &GatewayEvent, ctx: &PluginContext) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called at shutdown for cleanup.
    async fn on_shutdown(&self) -> anyhow::Result<()> { Ok(()) }
}
```

### PluginRegistrar

Used during `on_register` to declare what a plugin needs:

```rust
pub struct PluginRegistrar {
    router: Router,
    subscriptions: Vec<EventType>,
}

impl PluginRegistrar {
    pub fn add_routes(&mut self, router: Router);
    pub fn subscribe(&mut self, event: EventType);

    /// Consumes the registrar, returns the router and subscriptions separately.
    pub fn build(self) -> (Router, Vec<EventType>) {
        (self.router, self.subscriptions)
    }
}
```

### State Injection for Plugin Routes

All plugin routers use `Router<()>` (no generic state parameter). State is passed via Axum `Extension` layers. When a plugin registers routes, `main.rs` layers `Extension(plugin_context.clone())` onto the merged router. This matches the existing codebase pattern where handlers extract state via `Extension<Arc<...>>`.

Plugins that need their own internal state (e.g., `DashboardPlugin` with `AdminSessionStore`) store it in `Arc` fields and clone those into route handlers via closures or additional `Extension` layers during `on_register`.

```rust
// Example: DashboardPlugin stores its own state in Arc
pub struct DashboardPlugin {
    admin_store: Arc<AdminSessionStore>,
}

impl Plugin for DashboardPlugin {
    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        let store = self.admin_store.clone();
        registrar.add_routes(
            Router::new()
                .route("/admin/login", post(handlers::login))
                .route("/admin/overview", get(handlers::overview))
                // ...
                .layer(Extension(store))
        );
        Ok(())
    }
}
```

### PluginContext — Controlled State Access

Plugins access gateway state through a defined API, not directly. Internally holds multiple `Arc` fields matching the existing codebase structure (no new `AppState` struct needed):

```rust
#[derive(Clone)]
pub struct PluginContext {
    session_store: Arc<SessionStore>,
    key_logger: Arc<KeyLogger>,
    config: Arc<Config>,
    request_counter: Arc<AtomicU64>,
    models_cache: Arc<ModelsCache>,
    event_bus: Arc<EventBus>,
    start_time: Instant,
}

impl PluginContext {
    // Read-only accessors
    pub fn active_session_count(&self) -> usize;
    pub fn total_request_count(&self) -> u64;
    pub fn uptime(&self) -> Duration;
    pub fn list_keys(&self) -> Vec<KeyInfo>;

    // Log/stats reading
    pub fn read_logs(&self, filter: LogFilter) -> Vec<LogEntry>;
    pub fn aggregate_costs(&self, period: CostPeriod) -> CostBreakdown;

    // Write operations
    pub fn log_event(&self, event: AuditEvent);
}
```

### Event System

#### EventType Enum

```rust
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
```

#### GatewayEvent

```rust
pub enum GatewayEvent {
    // Lifecycle
    ServerStarted { port: u16 },
    ServerShutdown,

    // Request lifecycle
    RequestReceived { key_id: String, path: String, method: String },
    RequestCompleted { key_id: String, path: String, status: u16, duration: Duration },

    // Session
    SessionStarted { session_id: String, model: String, key_id: String },
    SessionCompleted { session_id: String, token_usage: TokenUsage },
    SessionDeleted { session_id: String },

    // Cost
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
```

#### EventBus

Registration happens only at startup before the server accepts connections, so no concurrent mutation concerns. Event dispatch uses `tokio::spawn` for true fire-and-forget — a slow plugin does not block others.

```rust
pub struct EventBus {
    subscribers: HashMap<EventType, Vec<(String, Arc<dyn Plugin>)>>,
}

impl EventBus {
    pub fn register(&mut self, plugin: Arc<dyn Plugin>, subscriptions: &[EventType]) {
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
```

#### Error Handling for Events

- `on_event` returns `anyhow::Result<()>`. Errors are logged with `tracing::error!` and swallowed — one plugin's failure does not affect others.
- Panics in spawned tasks are caught by Tokio's default panic handler and logged.

#### Plugin Event Usage

| Plugin | Subscribed Events | Purpose |
|--------|-------------------|---------|
| Dashboard | all | Collects statistics, displays in UI |
| Swagger | none | Only registers routes |
| CF Tunnel | `ServerStarted`, `ServerShutdown` | Opens/closes tunnel |
| Telegram | `SessionCompleted`, `CostRecorded` | Sends notifications |

### Config System

#### claudeway.toml

```toml
[server]
port = 3000
workdir = "/tmp/claude-tasks"
log_dir = "./logs"

[plugins.dashboard]
enabled = true

[plugins.swagger]
enabled = true

[plugins.cf_tunnel]
enabled = true
tunnel_token = "eyJ..."

[plugins.telegram]
enabled = false
bot_token = "123456:ABC..."
allowed_chat_ids = [12345, 67890]
```

#### Config file struct

`PluginConfig` is a new struct for the config file — it does not replace the existing `Config` (CLI args). The two are merged at startup: `Config` fields take precedence over `PluginConfig.server` fields. Plugin-specific config lives only in `PluginConfig.plugins`.

```rust
#[derive(Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub plugins: HashMap<String, toml::Value>,
}
```

Each plugin defines its own config struct and deserializes from `plugins.{name}`:

```rust
#[derive(Deserialize)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub allowed_chat_ids: Vec<i64>,
}
```

#### Config Discovery & Precedence

Merge order (last wins): **defaults → config file → environment variables → CLI flags**

Config file search order:
1. `--config <path>` (explicit)
2. `./claudeway.toml` (current directory)
3. None found → use CLI-only mode (backward compatible)

#### CLI Struct Updates

```rust
#[derive(Parser)]
struct Cli {
    // ... existing fields ...

    /// Path to config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Disable specific plugins (overrides config file)
    #[arg(long, value_delimiter = ',')]
    disable_plugin: Vec<String>,
}
```

If no config file is present, existing CLI-only behavior is preserved — no breaking changes.

#### New Dependencies

```toml
[dependencies]
toml = "0.8"           # config file parsing
async-trait = "0.1"    # #[async_trait] for Plugin trait
```

## Migration Plan

### File Reorganization

Existing admin/dashboard/swagger code is wrapped in plugin trait implementations. `main.rs` changes significantly — plugin loading replaces inline feature-gated route setup.

```
src/
├── plugin.rs              # Plugin trait, PluginContext, EventBus, PluginRegistrar, EventType, GatewayEvent
├── config.rs              # Updated: config file loading + CLI + merge logic
├── plugins/
│   ├── mod.rs             # plugin_registry() — builds Vec<Arc<dyn Plugin>> from config
│   ├── dashboard/         # #[cfg(feature = "dashboard")]
│   │   ├── mod.rs         # DashboardPlugin impl
│   │   ├── auth.rs        # ← admin_auth.rs
│   │   ├── models.rs      # ← admin_models.rs
│   │   ├── stats.rs       # ← admin_stats.rs
│   │   └── handlers.rs    # ← handlers/admin.rs + dashboard.rs
│   └── swagger/           # #[cfg(feature = "swagger")]
│       └── mod.rs         # SwaggerPlugin impl
├── main.rs                # Simplified: load config, build plugins, merge routes, wire events
└── ...                    # Core handler files unchanged (health, models, task, session)
```

### plugin_registry() Function

```rust
pub fn plugin_registry(config: &PluginConfig, ctx: &PluginContext) -> Vec<Arc<dyn Plugin>> {
    let mut plugins: Vec<Arc<dyn Plugin>> = Vec::new();

    #[cfg(feature = "dashboard")]
    {
        if is_plugin_enabled(config, "dashboard") {
            plugins.push(Arc::new(DashboardPlugin::new(ctx)));
        }
    }

    #[cfg(feature = "swagger")]
    {
        if is_plugin_enabled(config, "swagger") {
            plugins.push(Arc::new(SwaggerPlugin));
        }
    }

    plugins
}

fn is_plugin_enabled(config: &PluginConfig, name: &str) -> bool {
    config.plugins
        .get(name)
        .and_then(|v| v.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true) // enabled by default if compiled in
}
```

### main.rs Integration

```rust
let plugin_config = load_plugin_config(&cli)?;  // from claudeway.toml
let mut event_bus = EventBus::new();

// Build plugins from config + feature flags
let plugins = plugin_registry(&plugin_config, &cli);

let mut app = Router::new().merge(core_routes());

for plugin in &plugins {
    let mut registrar = PluginRegistrar::new();
    plugin.on_register(&mut registrar)?;
    let (router, subscriptions) = registrar.build();
    app = app.merge(router);
    event_bus.register(plugin.clone(), &subscriptions);
}

// Wrap EventBus in Arc and store in PluginContext
let event_bus = Arc::new(event_bus);
let plugin_ctx = PluginContext::new(
    session_store, key_logger, config, request_counter, models_cache, event_bus,
);

// Layer PluginContext as Extension for all route handlers (core + plugin)
app = app.layer(Extension(plugin_ctx.clone()));

// Emit initial event after server starts listening
// event_bus is accessible via plugin_ctx.event_bus in request handlers
```

Request handlers emit events through `PluginContext`:

```rust
// In a handler like task.rs or session.rs:
async fn create_task(Extension(ctx): Extension<PluginContext>, ...) {
    // ... handle request ...
    ctx.emit(GatewayEvent::CostRecorded { key_id, model, cost });
}
```

`PluginContext::emit` delegates to the internal `Arc<EventBus>`.

## Cargo Features

Feature flags are preserved but now gate plugin structs:

```toml
[features]
default = []
swagger = ["utoipa-swagger-ui"]
dashboard = ["rust-embed", "dep:mime_guess"]
```

Building remains the same:

```bash
cargo build                      # Core API only
cargo build --features swagger   # + Swagger UI plugin
cargo build --features dashboard # + Dashboard plugin
cargo build --all-features       # Everything
```

## Future Plugins (Not in Scope)

These are motivation for the plugin system but will be implemented separately:

- **cf-tunnel** — Cloudflare Tunnel integration, auto-exposes gateway
- **telegram** — Telegram bot adapter for interacting with Claude via chat
- Other adapters (Slack, Discord, etc.)

## Design Notes

- **Async initialization:** `on_register` is synchronous. Plugins needing async init (e.g., CF Tunnel opening a connection, Telegram verifying bot token) should defer that work to the `ServerStarted` event handler.
- **`std::time::Duration`** is used in event payloads. `TokenUsage` is the existing struct from `src/models.rs`.

## Non-Goals

- Dynamic loading of `.so`/`.dylib` files — plugins are compiled in
- Plugin-to-plugin communication — plugins only interact via gateway events
- Plugin marketplace or distribution — plugins live in the main repo
- Plugin initialization ordering — not needed for current scope; can be added later with a `priority()` method if required
