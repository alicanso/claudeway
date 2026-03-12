#![allow(dead_code)]

use axum::Router;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::handlers::models::ModelsCache;
use crate::logging::KeyLogger;
use crate::models::TokenUsage;
use crate::session::SessionStore;

// ── Plugin Trait ──
// Uses Pin<Box<dyn Future>> for async methods to remain dyn-compatible.
// All plugins must be Send + Sync + 'static so they can be shared across threads.

pub trait Plugin: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()>;
    fn on_event(
        &self,
        _event: &GatewayEvent,
        _ctx: &PluginContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
    fn on_shutdown(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
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

type PluginEntry = (String, Arc<dyn Plugin>);

pub struct EventBus {
    subscribers: HashMap<EventType, Vec<PluginEntry>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

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
        let event = GatewayEvent::ServerShutdown;
        assert_eq!(event.event_type(), EventType::ServerShutdown);
        // Just verify EventBus can be created and events can be checked
        assert!(bus.subscribers.is_empty());
    }
}
