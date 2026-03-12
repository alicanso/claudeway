# Admin Dashboard Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an optional admin dashboard to Claudeway — a Svelte SPA embedded in the binary via `--features dashboard`, served at `/dashboard`.

**Architecture:** Rust backend gets admin API endpoints under `/admin` with session-cookie auth. Svelte + Vite SPA is built by `build.rs`, embedded with `rust-embed`, and served as static files. Everything gated behind `#[cfg(feature = "dashboard")]`.

**Tech Stack:** Rust (axum, rust-embed, dashmap), Svelte 5 + Vite, svelte-spa-router, Chart.js

**Spec:** `docs/superpowers/specs/2026-03-12-admin-dashboard-design.md`

---

## File Structure

### New Rust Files
- `src/admin_auth.rs` — Admin session-cookie auth: login endpoint, token storage (`DashMap<String, AdminSession>`), cookie middleware, lazy cleanup
- `src/admin_stats.rs` — Log parsing and aggregation utilities (used by admin handlers for costs, keys, overview)
- `src/handlers/admin.rs` — Admin API handlers: overview, sessions list, session detail, logs, keys, costs
- `src/admin_models.rs` — Request/response types for all admin endpoints
- `build.rs` — Conditional Svelte build pipeline (only when `dashboard` feature active)
- `src/dashboard.rs` — Static file serving with `rust-embed`, `index.html` SPA fallback

### Modified Rust Files
- `Cargo.toml` — Add `rust-embed` dep, `dashboard` feature flag
- `src/config.rs` — Add `admin_key_id: String` field, update `parse_keys` and `load`
- `src/session.rs` — Add `key_id: String` to `SessionMeta`, add `list_all()` method to `SessionStore`
- `src/logging.rs` — Add `Deserialize` to log structs, add `log_dir()` accessor
- `src/handlers/mod.rs` — Add `pub mod admin;`
- `src/handlers/session.rs` — Pass `key_id` when creating `SessionMeta`
- `src/handlers/task.rs` — Increment global request counter
- `src/main.rs` — Wire admin routes, dashboard serving, request counter, admin auth state

### New Svelte Files (all under `dashboard/`)
- `package.json`, `vite.config.ts`, `tsconfig.json` — Project scaffolding
- `src/App.svelte` — Root component with router
- `src/main.ts` — Entry point
- `src/lib/api.ts` — API client (`fetch` wrapper with `credentials: 'same-origin'`)
- `src/lib/stores.ts` — Svelte stores for auth state
- `src/routes/Login.svelte` — Login form
- `src/routes/Overview.svelte` — Dashboard overview with stat cards + charts
- `src/routes/Sessions.svelte` — Sessions list with pagination/filtering
- `src/routes/SessionDetail.svelte` — Single session detail view
- `src/routes/Logs.svelte` — Log viewer with polling
- `src/routes/Keys.svelte` — API key stats
- `src/routes/Costs.svelte` — Cost analytics charts
- `src/lib/components/Navbar.svelte` — Navigation bar
- `src/lib/components/StatCard.svelte` — Reusable stat card
- `src/lib/components/EmptyState.svelte` — Empty state placeholder
- `index.html` — HTML entry point

---

## Chunk 1: Rust Backend — Config, Session, and Admin Auth

### Task 1: Add `admin_key_id` to Config

**Files:**
- Modify: `src/config.rs:34-43` (Config struct)
- Modify: `src/config.rs:46-68` (Config::load)
- Modify: `src/config.rs:70-104` (Config::parse_keys)

- [ ] **Step 1: Write failing test for admin_key_id in parse_keys**

Add to `src/config.rs` tests:

```rust
#[test]
fn test_parse_keys_returns_admin_key_id() {
    let (keys, admin_key_id) = Config::parse_keys_with_admin("admin:sk-001,ci:sk-002").unwrap();
    assert_eq!(admin_key_id, "admin");
    assert_eq!(keys.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_parse_keys_returns_admin_key_id -- --nocapture`
Expected: FAIL — `parse_keys_with_admin` doesn't exist

- [ ] **Step 3: Implement parse_keys_with_admin and update Config**

In `src/config.rs`:

1. Add `admin_key_id: String` to `Config` struct
2. Create `parse_keys_with_admin` that returns `(HashMap<String, String>, String)` — captures first key_id before HashMap insert:

```rust
pub fn parse_keys_with_admin(raw: &str) -> anyhow::Result<(HashMap<String, String>, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
    }

    let mut map = HashMap::new();
    let mut admin_key_id: Option<String> = None;

    for entry in trimmed.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid key format: expected 'key_id:key_value', got '{entry}'"
            ));
        }
        let id = parts[0].trim();
        let value = parts[1].trim();
        if id.is_empty() {
            return Err(anyhow::anyhow!("Key ID cannot be empty"));
        }
        if value.is_empty() {
            return Err(anyhow::anyhow!("Key value cannot be empty"));
        }
        if admin_key_id.is_none() {
            admin_key_id = Some(id.to_string());
        }
        map.insert(value.to_string(), id.to_string());
    }

    if map.is_empty() {
        return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
    }

    Ok((map, admin_key_id.unwrap()))
}
```

3. Update `Config::load` to use `parse_keys_with_admin` and set `admin_key_id`:

```rust
let (api_keys, admin_key_id, generated_key) = match cli.keys {
    Some(raw) => {
        let (keys, admin) = Self::parse_keys_with_admin(&raw)?;
        (keys, admin, None)
    }
    None => {
        let secret = generate_secret();
        let mut map = HashMap::new();
        map.insert(secret.clone(), "default".to_string());
        (map, "default".to_string(), Some(secret))
    }
};
```

4. Keep existing `parse_keys` as-is (used in existing tests) or redirect it to call `parse_keys_with_admin`.

- [ ] **Step 4: Run all config tests**

Run: `cargo test config::tests -- --nocapture`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add admin_key_id tracking for first API key"
```

---

### Task 2: Add `key_id` to SessionMeta and `list_all` to SessionStore

**Files:**
- Modify: `src/session.rs:12-24` (SessionMeta struct)
- Modify: `src/session.rs:27-79` (SessionStore impl)
- Modify: `src/handlers/session.rs:56-68` (start_session — pass key_id)

- [ ] **Step 1: Write failing test for key_id in SessionMeta**

Add to `src/session.rs` tests:

```rust
#[test]
fn test_session_stores_key_id() {
    let store = SessionStore::new();
    let session_id = Uuid::new_v4();
    let now = Utc::now();

    let meta = SessionMeta {
        session_id,
        claude_session_id: None,
        created_at: now,
        last_used: now,
        model: None,
        system_prompt: None,
        workdir: PathBuf::from("/tmp"),
        auto_workdir: false,
        task_count: 0,
        tokens: TokenUsage::default(),
        cost_usd: 0.0,
        key_id: "admin".to_string(),
    };

    store.insert(meta);
    let retrieved = store.get(&session_id).unwrap();
    assert_eq!(retrieved.key_id, "admin");
}

#[test]
fn test_list_all_sessions() {
    let store = SessionStore::new();
    let now = Utc::now();

    for i in 0..3 {
        let meta = SessionMeta {
            session_id: Uuid::new_v4(),
            claude_session_id: None,
            created_at: now,
            last_used: now,
            model: Some("sonnet".to_string()),
            system_prompt: None,
            workdir: PathBuf::from("/tmp"),
            auto_workdir: false,
            task_count: i,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
            key_id: "admin".to_string(),
        };
        store.insert(meta);
    }

    let all = store.list_all();
    assert_eq!(all.len(), 3);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test session::tests -- --nocapture`
Expected: FAIL — `key_id` field missing, `list_all` doesn't exist

- [ ] **Step 3: Add key_id field and list_all method**

In `src/session.rs`:

1. Add `pub key_id: String` to `SessionMeta`
2. Add to `SessionStore` impl:

```rust
pub fn list_all(&self) -> Vec<SessionMeta> {
    self.sessions.iter().map(|entry| entry.value().clone()).collect()
}
```

3. Update existing tests to include `key_id: "test".to_string()` in all `SessionMeta` constructions.

- [ ] **Step 4: Update handlers to pass key_id**

In `src/handlers/session.rs`, update `start_session`:

```rust
// Change Extension(_key_id) to Extension(key_id)
Extension(key_id): Extension<KeyId>,
```

And in the `SessionMeta` construction:

```rust
key_id: key_id.0.clone(),
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -- --nocapture`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src/session.rs src/handlers/session.rs
git commit -m "feat(session): add key_id tracking and list_all method"
```

---

### Task 3: Admin Session-Cookie Auth

**Files:**
- Create: `src/admin_auth.rs`
- Modify: `src/main.rs:1-20` (add mod declaration)

- [ ] **Step 1: Write failing test for admin auth**

Create `src/admin_auth.rs` with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_admin_session() {
        let store = AdminSessionStore::new();
        let token = store.create_session();
        assert!(store.validate(&token));
    }

    #[test]
    fn test_invalid_token_rejected() {
        let store = AdminSessionStore::new();
        assert!(!store.validate("invalid-token"));
    }

    #[test]
    fn test_expired_session_rejected() {
        let store = AdminSessionStore::new();
        // Insert a session that expired 2 hours ago
        let token = uuid::Uuid::new_v4().to_string();
        store.sessions.insert(token.clone(), AdminSession {
            expires_at: chrono::Utc::now() - chrono::Duration::hours(2),
        });
        assert!(!store.validate(&token));
    }

    #[test]
    fn test_cleanup_removes_expired() {
        let store = AdminSessionStore::new();
        // Insert expired session
        let expired_token = uuid::Uuid::new_v4().to_string();
        store.sessions.insert(expired_token.clone(), AdminSession {
            expires_at: chrono::Utc::now() - chrono::Duration::hours(2),
        });
        // Insert valid session
        let valid_token = store.create_session();

        store.cleanup_expired();
        assert_eq!(store.sessions.len(), 1);
        assert!(store.validate(&valid_token));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test admin_auth::tests -- --nocapture`
Expected: FAIL — module doesn't compile yet

- [ ] **Step 3: Implement AdminSessionStore**

Write `src/admin_auth.rs`:

```rust
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use uuid::Uuid;

const SESSION_TTL_HOURS: i64 = 1;

#[derive(Debug, Clone)]
pub struct AdminSession {
    pub expires_at: DateTime<Utc>,
}

pub struct AdminSessionStore {
    pub sessions: DashMap<String, AdminSession>,
}

impl AdminSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn create_session(&self) -> String {
        self.cleanup_expired();
        let token = Uuid::new_v4().to_string();
        let session = AdminSession {
            expires_at: Utc::now() + Duration::hours(SESSION_TTL_HOURS),
        };
        self.sessions.insert(token.clone(), session);
        token
    }

    pub fn validate(&self, token: &str) -> bool {
        if let Some(session) = self.sessions.get(token) {
            session.expires_at > Utc::now()
        } else {
            false
        }
    }

    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        self.sessions.retain(|_, session| session.expires_at > now);
    }
}
```

- [ ] **Step 4: Add mod declaration in main.rs**

In `src/main.rs`, add:

```rust
#[cfg(feature = "dashboard")]
mod admin_auth;
```

- [ ] **Step 5: Run tests**

Run: `cargo test admin_auth::tests -- --nocapture`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add src/admin_auth.rs src/main.rs
git commit -m "feat(admin): add session-cookie auth store with TTL and cleanup"
```

---

### Task 4: Add global request counter

**Files:**
- Modify: `src/main.rs` (add AtomicU64 counter, pass as Extension)
- Modify: `src/handlers/task.rs` (increment counter)
- Modify: `src/handlers/session.rs` (increment counter on continue_session)

- [ ] **Step 1: Add counter to main.rs**

In `src/main.rs`, add:

```rust
use std::sync::atomic::AtomicU64;
```

After `let start_time = ...`:

```rust
let request_counter = Arc::new(AtomicU64::new(0));
```

Add to protected_routes layers:

```rust
.layer(Extension(request_counter.clone()))
```

- [ ] **Step 2: Increment in task handler**

In `src/handlers/task.rs`, add parameter and increment:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

pub async fn create_task(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Json(req): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, AppError> {
    request_counter.fetch_add(1, Ordering::Relaxed);
    // ... rest unchanged
```

- [ ] **Step 3: Increment in session continue handler**

In `src/handlers/session.rs`, add same pattern to `continue_session`.

- [ ] **Step 4: Run all tests and verify build**

Run: `cargo build && cargo test -- --nocapture`
Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/handlers/task.rs src/handlers/session.rs
git commit -m "feat: add global AtomicU64 request counter"
```

---

### Task 5: Admin API models

**Files:**
- Create: `src/admin_models.rs`

- [ ] **Step 1: Create admin_models.rs with all request/response types**

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// --- Login ---

#[derive(Debug, Deserialize)]
pub struct AdminLoginRequest {
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct AdminLoginResponse {
    pub success: bool,
}

// --- Overview ---

#[derive(Debug, Serialize)]
pub struct OverviewResponse {
    pub uptime_secs: u64,
    pub total_requests: u64,
    pub active_sessions: u64,
    pub total_cost_usd: f64,
    pub models_breakdown: Vec<ModelBreakdown>,
}

#[derive(Debug, Serialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub request_count: u64,
    pub cost_usd: f64,
}

// --- Sessions ---

#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    pub page: Option<u64>,
    pub limit: Option<u64>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionsListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: u64,
    pub page: u64,
    pub limit: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u32,
    pub cost_usd: f64,
    pub key_id: String,
}

#[derive(Debug, Serialize)]
pub struct SessionDetailResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u32,
    pub cost_usd: f64,
    pub key_id: String,
    pub tokens: crate::models::TokenUsage,
    pub workdir: String,
}

// --- Logs ---

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub key_id: Option<String>,
    pub date: Option<String>,
    pub after: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub entries: Vec<serde_json::Value>,
    pub total: u64,
}

// --- Keys ---

#[derive(Debug, Serialize)]
pub struct KeysResponse {
    pub keys: Vec<KeyStats>,
}

#[derive(Debug, Serialize)]
pub struct KeyStats {
    pub key_id: String,
    pub total_requests: u64,
    pub total_cost_usd: f64,
}

// --- Costs ---

#[derive(Debug, Deserialize)]
pub struct CostsQuery {
    pub group_by: Option<String>, // "daily", "weekly", "monthly"
}

#[derive(Debug, Serialize)]
pub struct CostsResponse {
    pub data: Vec<CostEntry>,
}

#[derive(Debug, Serialize)]
pub struct CostEntry {
    pub period: String,
    pub cost_usd: f64,
    pub request_count: u64,
    pub by_model: Vec<ModelBreakdown>,
    pub by_key: Vec<KeyCost>,
}

#[derive(Debug, Serialize)]
pub struct KeyCost {
    pub key_id: String,
    pub cost_usd: f64,
}
```

- [ ] **Step 2: Add mod declaration**

In `src/main.rs`:

```rust
#[cfg(feature = "dashboard")]
mod admin_models;
```

- [ ] **Step 3: Verify build**

Run: `cargo build --features dashboard`
Expected: Compiles (will fail if dashboard feature deps aren't added yet — that's fine, just verify with `cargo check`)

- [ ] **Step 4: Commit**

```bash
git add src/admin_models.rs src/main.rs
git commit -m "feat(admin): add request/response models for admin API"
```

---

### Task 5b: Log parsing and aggregation utilities

**Files:**
- Create: `src/admin_stats.rs`
- Modify: `src/main.rs` (add mod declaration)

- [ ] **Step 1: Create admin_stats.rs**

This module extracts log-parsing logic from handlers, keeping `handlers/admin.rs` focused on HTTP concerns.

```rust
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::admin_models::{CostEntry, KeyCost, ModelBreakdown};

/// Parse all log files and return per-model breakdown (request count + cost)
pub fn get_models_breakdown(log_dir: &Path) -> Vec<ModelBreakdown> {
    let mut model_map: HashMap<String, (u64, f64)> = HashMap::new();

    walk_log_entries(log_dir, |value, _key_id| {
        let cost = value.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        if cost > 0.0 {
            let entry = model_map.entry(model).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += cost;
        }
    });

    model_map
        .into_iter()
        .map(|(model, (count, cost))| ModelBreakdown {
            model,
            request_count: count,
            cost_usd: cost,
        })
        .collect()
}

/// Aggregate costs by period (daily/weekly/monthly)
pub fn aggregate_costs(log_dir: &Path, group_by: &str) -> Vec<CostEntry> {
    // BTreeMap key: period string, value: (cost, count, by_model<model, (count, cost)>, by_key<key, cost>)
    let mut cost_map: BTreeMap<String, (f64, u64, HashMap<String, (u64, f64)>, HashMap<String, f64>)> =
        BTreeMap::new();

    walk_log_entries(log_dir, |value, key_id| {
        let cost = value.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if cost == 0.0 {
            return;
        }

        let timestamp = value.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let period = match group_by {
            "weekly" => {
                // Parse date and compute ISO week: "YYYY-Wxx"
                if timestamp.len() >= 10 {
                    if let Ok(date) = chrono::NaiveDate::parse_from_str(&timestamp[..10], "%Y-%m-%d") {
                        use chrono::Datelike;
                        format!("{}-W{:02}", date.iso_week().year(), date.iso_week().week())
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            "monthly" => {
                if timestamp.len() >= 7 {
                    timestamp[..7].to_string()
                } else {
                    return;
                }
            }
            _ => {
                // daily
                if timestamp.len() >= 10 {
                    timestamp[..10].to_string()
                } else {
                    return;
                }
            }
        };

        let model = value.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        let entry = cost_map.entry(period).or_insert_with(|| {
            (0.0, 0, HashMap::new(), HashMap::new())
        });
        entry.0 += cost;
        entry.1 += 1;
        let model_entry = entry.2.entry(model).or_insert((0, 0.0));
        model_entry.0 += 1;
        model_entry.1 += cost;
        *entry.3.entry(key_id.to_string()).or_insert(0.0) += cost;
    });

    cost_map
        .into_iter()
        .map(|(period, (cost, count, by_model, by_key))| CostEntry {
            period,
            cost_usd: cost,
            request_count: count,
            by_model: by_model
                .into_iter()
                .map(|(model, (req_count, cost))| ModelBreakdown {
                    model,
                    request_count: req_count,
                    cost_usd: cost,
                })
                .collect(),
            by_key: by_key
                .into_iter()
                .map(|(key_id, cost)| KeyCost { key_id, cost_usd: cost })
                .collect(),
        })
        .collect()
}

/// Get per-key stats (total requests and cost) from log files
pub fn get_keys_stats(log_dir: &Path, key_ids: &[String]) -> Vec<crate::admin_models::KeyStats> {
    let mut stats: HashMap<String, (u64, f64)> = HashMap::new();
    for key_id in key_ids {
        stats.insert(key_id.clone(), (0, 0.0));
    }

    walk_log_entries(log_dir, |value, key_id| {
        let entry = stats.entry(key_id.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        if let Some(cost) = value.get("cost_usd").and_then(|v| v.as_f64()) {
            entry.1 += cost;
        }
    });

    stats
        .into_iter()
        .map(|(key_id, (reqs, cost))| crate::admin_models::KeyStats {
            key_id,
            total_requests: reqs,
            total_cost_usd: cost,
        })
        .collect()
}

/// Walk all log entries across all key directories, calling f(value, key_id) for each
fn walk_log_entries(log_dir: &Path, mut f: impl FnMut(&serde_json::Value, &str)) {
    let Ok(rd) = std::fs::read_dir(log_dir) else { return };
    for dir_entry in rd.filter_map(|e| e.ok()) {
        let dir_path = dir_entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let key_id = dir_path.file_name().unwrap().to_string_lossy().to_string();

        let Ok(files) = std::fs::read_dir(&dir_path) else { continue };
        for file_entry in files.filter_map(|e| e.ok()) {
            let Ok(content) = std::fs::read_to_string(file_entry.path()) else { continue };
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    f(&value, &key_id);
                }
            }
        }
    }
}
```

- [ ] **Step 2: Add mod declaration in main.rs**

```rust
#[cfg(feature = "dashboard")]
mod admin_stats;
```

- [ ] **Step 3: Verify build**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/admin_stats.rs src/main.rs
git commit -m "feat(admin): extract log parsing into admin_stats module"
```

---

### Task 6: Admin API handlers

**Files:**
- Create: `src/handlers/admin.rs`
- Modify: `src/handlers/mod.rs` (add pub mod admin)
- Modify: `src/logging.rs` (add Deserialize to log structs, add log_dir accessor)

- [ ] **Step 1: Add Deserialize to log structs**

In `src/logging.rs`:

1. Add `Deserialize` to `ClaudeInvocationLog`:

```rust
#[derive(Serialize, Deserialize)]
pub struct ClaudeInvocationLog { ... }
```

2. Add `log_dir` accessor:

```rust
pub fn log_dir(&self) -> &Path {
    &self.log_dir
}
```

- [ ] **Step 2: Create admin handlers**

Create `src/handlers/admin.rs`:

```rust
use axum::extract::{Extension, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::admin_auth::AdminSessionStore;
use crate::admin_models::*;
use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::logging::KeyLogger;
use crate::session::SessionStore;

/// POST /admin/login
pub async fn login(
    Extension(config): Extension<Arc<Config>>,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Json(req): Json<AdminLoginRequest>,
) -> Result<Response, AppError> {
    // Check if the provided key matches the admin key
    let key_id = config.api_keys.get(&req.key);
    match key_id {
        Some(id) if *id == config.admin_key_id => {
            let token = admin_sessions.create_session();

            // Determine if we should set Secure flag
            // In production (non-localhost), always use Secure
            let cookie_value = format!(
                "admin_token={}; HttpOnly; SameSite=Strict; Path=/admin; Max-Age=3600",
                token
            );

            let mut headers = HeaderMap::new();
            headers.insert("Set-Cookie", cookie_value.parse().unwrap());
            headers.insert("Content-Type", "application/json".parse().unwrap());

            let body = serde_json::to_string(&AdminLoginResponse { success: true }).unwrap();

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Set-Cookie", cookie_value)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(body))
                .unwrap())
        }
        _ => Err(ApiError::unauthorized()),
    }
}

/// Extract and validate admin token from cookie
fn validate_admin_cookie(headers: &HeaderMap, store: &AdminSessionStore) -> Result<(), AppError> {
    let cookie_header = headers
        .get("Cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = cookie_header
        .split(';')
        .filter_map(|c| {
            let c = c.trim();
            c.strip_prefix("admin_token=")
        })
        .next();

    match token {
        Some(t) if store.validate(t) => Ok(()),
        _ => Err(ApiError::unauthorized()),
    }
}

/// GET /admin/overview
pub async fn overview(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(start_time): Extension<Arc<Instant>>,
    Extension(request_counter): Extension<Arc<AtomicU64>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
) -> Result<Json<OverviewResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let sessions = store.list_all();
    let total_cost: f64 = sessions.iter().map(|s| s.cost_usd).sum();
    let models_breakdown = crate::admin_stats::get_models_breakdown(logger.log_dir());

    Ok(Json(OverviewResponse {
        uptime_secs: start_time.elapsed().as_secs(),
        total_requests: request_counter.load(Ordering::Relaxed),
        active_sessions: sessions.len() as u64,
        total_cost_usd: total_cost,
        models_breakdown,
    }))
}

/// GET /admin/sessions
pub async fn list_sessions(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<SessionsListResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).min(100);

    let mut sessions: Vec<_> = store.list_all();

    // Filter by model
    if let Some(ref model) = query.model {
        sessions.retain(|s| s.model.as_deref() == Some(model.as_str()));
    }

    let total = sessions.len() as u64;

    // Sort by created_at descending
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // Paginate
    let start = ((page - 1) * limit) as usize;
    let page_sessions: Vec<SessionSummary> = sessions
        .into_iter()
        .skip(start)
        .take(limit as usize)
        .map(|s| SessionSummary {
            session_id: s.session_id.to_string(),
            created_at: s.created_at,
            last_used: s.last_used,
            model: s.model,
            task_count: s.task_count,
            cost_usd: s.cost_usd,
            key_id: s.key_id,
        })
        .collect();

    Ok(Json(SessionsListResponse {
        sessions: page_sessions,
        total,
        page,
        limit,
    }))
}

/// GET /admin/sessions/:id
pub async fn get_session_detail(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetailResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let session_id = uuid::Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("Invalid session ID"))?;

    let session = store
        .get(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    Ok(Json(SessionDetailResponse {
        session_id: session.session_id.to_string(),
        created_at: session.created_at,
        last_used: session.last_used,
        model: session.model,
        task_count: session.task_count,
        cost_usd: session.cost_usd,
        key_id: session.key_id,
        tokens: session.tokens,
        workdir: session.workdir.to_string_lossy().to_string(),
    }))
}

/// GET /admin/logs
pub async fn get_logs(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let log_dir = logger.log_dir();
    let mut entries: Vec<serde_json::Value> = Vec::new();

    // Determine which key directories to scan
    let key_dirs: Vec<_> = if let Some(ref key_id) = query.key_id {
        vec![log_dir.join(key_id)]
    } else {
        std::fs::read_dir(log_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default()
    };

    // Determine which month file to read
    let month_str = match query.date.as_deref() {
        Some(d) if d.len() >= 7 => d[..7].to_string(),
        _ => Utc::now().format("%Y-%m").to_string(),
    };

    for dir in key_dirs {
        if !dir.is_dir() {
            continue;
        }
        let log_file = dir.join(format!("{}.log", month_str));
        if !log_file.exists() {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&log_file) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    // Apply after filter
                    if let Some(ref after) = query.after {
                        if let Some(ts) = value.get("timestamp").and_then(|v| v.as_str()) {
                            if ts <= after.as_str() {
                                continue;
                            }
                        }
                    }
                    entries.push(value);
                }
            }
        }
    }

    // Sort by timestamp descending
    entries.sort_by(|a, b| {
        let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        ts_b.cmp(ts_a)
    });

    let limit = query.limit.unwrap_or(100) as usize;
    let total = entries.len() as u64;
    entries.truncate(limit);

    Ok(Json(LogsResponse { entries, total }))
}

/// GET /admin/keys
pub async fn get_keys(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
) -> Result<Json<KeysResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let log_dir = logger.log_dir();
    let key_ids: Vec<String> = config.key_ids().into_iter().cloned().collect();
    let keys = crate::admin_stats::get_keys_stats(log_dir, &key_ids);

    Ok(Json(KeysResponse { keys }))
}

/// GET /admin/costs
pub async fn get_costs(
    headers: HeaderMap,
    Extension(admin_sessions): Extension<Arc<AdminSessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Query(query): Query<CostsQuery>,
) -> Result<Json<CostsResponse>, AppError> {
    validate_admin_cookie(&headers, &admin_sessions)?;

    let log_dir = logger.log_dir();
    let group_by = query.group_by.as_deref().unwrap_or("daily");

    let data = crate::admin_stats::aggregate_costs(log_dir, group_by);

    Ok(Json(CostsResponse { data }))
}
```

- [ ] **Step 3: Add mod declaration and Deserialize to logging**

In `src/handlers/mod.rs`:

```rust
#[cfg(feature = "dashboard")]
pub mod admin;
```

- [ ] **Step 4: Verify build**

Run: `cargo check`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add src/handlers/admin.rs src/handlers/mod.rs src/logging.rs
git commit -m "feat(admin): add all admin API endpoint handlers"
```

---

### Task 7: Wire admin routes into main.rs

**Files:**
- Modify: `src/main.rs`
- Modify: `Cargo.toml` (add rust-embed dep)

- [ ] **Step 1: Update Cargo.toml**

Add to `[dependencies]`:

```toml
rust-embed = { version = "8", optional = true }
```

Update `[features]`:

```toml
dashboard = ["rust-embed"]
```

- [ ] **Step 2: Wire admin routes in main.rs**

Add admin routes block in `src/main.rs` after protected_routes, conditionally compiled:

```rust
#[cfg(feature = "dashboard")]
let admin_session_store = Arc::new(admin_auth::AdminSessionStore::new());

#[cfg(feature = "dashboard")]
let admin_routes = {
    use axum::routing::{get, post};
    Router::new()
        .route("/admin/login", post(handlers::admin::login))
        .route("/admin/overview", get(handlers::admin::overview))
        .route("/admin/sessions", get(handlers::admin::list_sessions))
        .route("/admin/sessions/{id}", get(handlers::admin::get_session_detail))
        .route("/admin/logs", get(handlers::admin::get_logs))
        .route("/admin/keys", get(handlers::admin::get_keys))
        .route("/admin/costs", get(handlers::admin::get_costs))
        .layer(Extension(config.clone()))
        .layer(Extension(admin_session_store.clone()))
        .layer(Extension(start_time.clone()))
        .layer(Extension(request_counter.clone()))
        .layer(Extension(store.clone()))
        .layer(Extension(logger.clone()))
};

let mut app = Router::new()
    .merge(public_routes)
    .merge(protected_routes);

#[cfg(feature = "dashboard")]
{
    app = app.merge(admin_routes);
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build` (without dashboard feature — should still work)
Run: `cargo build --features dashboard` (with dashboard feature)
Expected: Both compile

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: wire admin routes with dashboard feature flag"
```

---

## Chunk 2: Build Pipeline and Dashboard Serving

### Task 8: Create build.rs for Svelte compilation

**Files:**
- Create: `build.rs`

- [ ] **Step 1: Create build.rs**

```rust
use std::process::Command;

fn main() {
    // Only build dashboard when the feature is enabled
    #[cfg(feature = "dashboard")]
    {
        let dashboard_dir = std::path::Path::new("dashboard");

        // Check if dashboard source exists
        if !dashboard_dir.join("package.json").exists() {
            println!("cargo:warning=Dashboard source not found at dashboard/. Skipping frontend build.");
            return;
        }

        let dist_dir = dashboard_dir.join("dist");
        let src_dir = dashboard_dir.join("src");

        // Check if rebuild is needed
        if dist_dir.exists() {
            // Check if any source file is newer than dist
            let dist_mtime = std::fs::metadata(&dist_dir)
                .and_then(|m| m.modified())
                .ok();

            let needs_rebuild = walkdir(&src_dir)
                .map(|src_mtime| {
                    dist_mtime.map_or(true, |d| src_mtime > d)
                })
                .unwrap_or(true);

            if !needs_rebuild {
                println!("cargo:warning=Dashboard dist is up to date, skipping build.");
                return;
            }
        }

        // Run npm install
        let status = Command::new("npm")
            .arg("install")
            .current_dir(dashboard_dir)
            .status()
            .expect("Failed to run npm install. Is Node.js installed?");

        if !status.success() {
            panic!("npm install failed");
        }

        // Run npm run build
        let status = Command::new("npm")
            .args(["run", "build"])
            .current_dir(dashboard_dir)
            .status()
            .expect("Failed to run npm run build");

        if !status.success() {
            panic!("npm run build failed");
        }

        // Tell cargo to rerun if dashboard source changes
        println!("cargo:rerun-if-changed=dashboard/src");
        println!("cargo:rerun-if-changed=dashboard/package.json");
        println!("cargo:rerun-if-changed=dashboard/vite.config.ts");
    }
}

/// Get the most recent modification time of any file in a directory
#[cfg(feature = "dashboard")]
fn walkdir(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut latest: Option<std::time::SystemTime> = None;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(t) = walkdir(&path) {
                    latest = Some(latest.map_or(t, |l: std::time::SystemTime| l.max(t)));
                }
            } else if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(mtime) = meta.modified() {
                    latest = Some(latest.map_or(mtime, |l: std::time::SystemTime| l.max(mtime)));
                }
            }
        }
    }
    latest
}
```

- [ ] **Step 2: Verify build without dashboard feature**

Run: `cargo build`
Expected: Compiles — build.rs does nothing without feature

- [ ] **Step 3: Commit**

```bash
git add build.rs
git commit -m "feat: add build.rs for conditional Svelte compilation"
```

---

### Task 9: Dashboard static file serving

**Files:**
- Create: `src/dashboard.rs`
- Modify: `src/main.rs` (add dashboard routes)

- [ ] **Step 1: Create dashboard.rs**

```rust
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "dashboard/dist/"]
struct DashboardAssets;

pub async fn serve_dashboard(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches("/dashboard").trim_start_matches('/');

    // Try exact file match first
    if let Some(file) = DashboardAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data.to_vec(),
        ).into_response();
    }

    // SPA fallback: serve index.html for all unmatched routes
    if let Some(file) = DashboardAssets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            file.data.to_vec(),
        ).into_response();
    }

    (StatusCode::NOT_FOUND, "Dashboard not found").into_response()
}
```

- [ ] **Step 2: Add mod and routes to main.rs**

In `src/main.rs`, add:

```rust
#[cfg(feature = "dashboard")]
mod dashboard;
```

And in the dashboard feature block, add dashboard static file routes:

```rust
#[cfg(feature = "dashboard")]
{
    app = app
        .merge(admin_routes)
        .route("/dashboard", get(dashboard::serve_dashboard))
        .route("/dashboard/*rest", get(dashboard::serve_dashboard));
}
```

- [ ] **Step 3: Add mime_guess to Cargo.toml**

```toml
mime_guess = { version = "2", optional = true }
```

Update feature:

```toml
dashboard = ["rust-embed", "mime_guess"]
```

- [ ] **Step 4: Commit**

```bash
git add src/dashboard.rs src/main.rs Cargo.toml
git commit -m "feat: add embedded dashboard static file serving with SPA fallback"
```

---

## Chunk 3: Svelte Frontend

### Task 10: Scaffold Svelte project

**Files:**
- Create: `dashboard/package.json`
- Create: `dashboard/vite.config.ts`
- Create: `dashboard/tsconfig.json`
- Create: `dashboard/index.html`
- Create: `dashboard/src/main.ts`
- Create: `dashboard/src/App.svelte`
- Create: `dashboard/src/vite-env.d.ts`

- [ ] **Step 1: Create package.json**

```json
{
  "name": "claudeway-dashboard",
  "private": true,
  "version": "0.0.1",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5",
    "svelte": "^5",
    "vite": "^6",
    "typescript": "^5"
  },
  "dependencies": {
    "svelte-spa-router": "^4",
    "chart.js": "^4"
  }
}
```

- [ ] **Step 2: Create vite.config.ts**

```typescript
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  base: '/dashboard/',
  build: {
    outDir: 'dist',
    emptyDirBeforeWrite: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/admin': 'http://localhost:3000',
    },
  },
})
```

- [ ] **Step 3: Create tsconfig.json**

```json
{
  "extends": "@sveltejs/vite-plugin-svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ESNext",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "resolveJsonModule": true,
    "allowJs": true,
    "checkJs": true,
    "isolatedModules": true
  },
  "include": ["src/**/*.ts", "src/**/*.svelte"]
}
```

- [ ] **Step 4: Create index.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Claudeway Dashboard</title>
</head>
<body>
  <div id="app"></div>
  <script type="module" src="/src/main.ts"></script>
</body>
</html>
```

- [ ] **Step 5: Create src/main.ts**

```typescript
import App from './App.svelte'
import { mount } from 'svelte'

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
```

- [ ] **Step 6: Create src/vite-env.d.ts**

```typescript
/// <reference types="svelte" />
/// <reference types="vite/client" />
```

- [ ] **Step 7: Create src/App.svelte**

```svelte
<script lang="ts">
  import Router from 'svelte-spa-router'
  import Navbar from './lib/components/Navbar.svelte'
  import Login from './routes/Login.svelte'
  import Overview from './routes/Overview.svelte'
  import Sessions from './routes/Sessions.svelte'
  import SessionDetail from './routes/SessionDetail.svelte'
  import Logs from './routes/Logs.svelte'
  import Keys from './routes/Keys.svelte'
  import Costs from './routes/Costs.svelte'
  import { isAuthenticated } from './lib/stores'

  const routes = {
    '/': Overview,
    '/sessions': Sessions,
    '/sessions/:id': SessionDetail,
    '/logs': Logs,
    '/keys': Keys,
    '/costs': Costs,
  }
</script>

{#if $isAuthenticated}
  <Navbar />
  <main>
    <Router {routes} />
  </main>
{:else}
  <Login />
{/if}

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: #0f172a;
    color: #e2e8f0;
  }
  main {
    max-width: 1200px;
    margin: 0 auto;
    padding: 24px;
  }
</style>
```

- [ ] **Step 8: Create placeholder route stubs**

Create minimal placeholder files so the Svelte build doesn't fail on missing imports. Each file should contain:

```svelte
<p>TODO</p>
```

Create these files:
- `dashboard/src/routes/Login.svelte`
- `dashboard/src/routes/Overview.svelte`
- `dashboard/src/routes/Sessions.svelte`
- `dashboard/src/routes/SessionDetail.svelte`
- `dashboard/src/routes/Logs.svelte`
- `dashboard/src/routes/Keys.svelte`
- `dashboard/src/routes/Costs.svelte`
- `dashboard/src/lib/components/Navbar.svelte`
- `dashboard/src/lib/stores.ts` (export: `export const isAuthenticated = writable(false)` with `import { writable } from 'svelte/store'`)

- [ ] **Step 9: Install deps and verify build**

```bash
cd dashboard && npm install && npm run build
```

Expected: Builds successfully, `dashboard/dist/` created with `index.html` and JS bundles

- [ ] **Step 9: Commit**

```bash
git add dashboard/
git commit -m "feat(dashboard): scaffold Svelte project with Vite"
```

---

### Task 11: API client and stores

**Files:**
- Create: `dashboard/src/lib/api.ts`
- Create: `dashboard/src/lib/stores.ts`

- [ ] **Step 1: Create api.ts**

```typescript
const BASE = '/admin'

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })

  if (res.status === 401) {
    // Session expired
    window.location.hash = '#/'
    isAuthenticated.set(false)
    throw new Error('Session expired')
  }

  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new Error(body.error || `HTTP ${res.status}`)
  }

  return res.json()
}

import { isAuthenticated } from './stores'

export async function login(key: string): Promise<boolean> {
  const res = await fetch(`${BASE}/login`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ key }),
  })
  if (res.ok) {
    isAuthenticated.set(true)
    return true
  }
  return false
}

export function getOverview() {
  return request('/overview')
}

export function getSessions(page = 1, limit = 20, model?: string) {
  const params = new URLSearchParams({ page: String(page), limit: String(limit) })
  if (model) params.set('model', model)
  return request(`/sessions?${params}`)
}

export function getSessionDetail(id: string) {
  return request(`/sessions/${id}`)
}

export function getLogs(opts: { key_id?: string; date?: string; after?: string; limit?: number } = {}) {
  const params = new URLSearchParams()
  if (opts.key_id) params.set('key_id', opts.key_id)
  if (opts.date) params.set('date', opts.date)
  if (opts.after) params.set('after', opts.after)
  if (opts.limit) params.set('limit', String(opts.limit))
  return request(`/logs?${params}`)
}

export function getKeys() {
  return request('/keys')
}

export function getCosts(groupBy = 'daily') {
  return request(`/costs?group_by=${groupBy}`)
}
```

- [ ] **Step 2: Create stores.ts**

```typescript
import { writable } from 'svelte/store'

export const isAuthenticated = writable(false)
```

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/lib/
git commit -m "feat(dashboard): add API client and auth store"
```

---

### Task 12: Shared components

**Files:**
- Create: `dashboard/src/lib/components/Navbar.svelte`
- Create: `dashboard/src/lib/components/StatCard.svelte`
- Create: `dashboard/src/lib/components/EmptyState.svelte`

- [ ] **Step 1: Create Navbar.svelte**

```svelte
<script lang="ts">
  import { isAuthenticated } from '../stores'
  import { link } from 'svelte-spa-router'

  function logout() {
    isAuthenticated.set(false)
  }
</script>

<nav>
  <div class="brand">Claudeway</div>
  <div class="links">
    <a href="/" use:link>Overview</a>
    <a href="/sessions" use:link>Sessions</a>
    <a href="/logs" use:link>Logs</a>
    <a href="/keys" use:link>Keys</a>
    <a href="/costs" use:link>Costs</a>
  </div>
  <button onclick={logout}>Logout</button>
</nav>

<style>
  nav {
    display: flex;
    align-items: center;
    padding: 12px 24px;
    background: #1e293b;
    border-bottom: 1px solid #334155;
  }
  .brand {
    font-weight: 700;
    font-size: 18px;
    color: #38bdf8;
    margin-right: 32px;
  }
  .links {
    display: flex;
    gap: 16px;
    flex: 1;
  }
  .links a {
    color: #94a3b8;
    text-decoration: none;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .links a:hover {
    color: #e2e8f0;
    background: #334155;
  }
  button {
    background: none;
    border: 1px solid #475569;
    color: #94a3b8;
    padding: 6px 12px;
    border-radius: 4px;
    cursor: pointer;
  }
  button:hover {
    color: #e2e8f0;
    border-color: #64748b;
  }
</style>
```

- [ ] **Step 2: Create StatCard.svelte**

```svelte
<script lang="ts">
  let { label, value, subtitle = '' }: { label: string; value: string | number; subtitle?: string } = $props()
</script>

<div class="card">
  <div class="label">{label}</div>
  <div class="value">{value}</div>
  {#if subtitle}
    <div class="subtitle">{subtitle}</div>
  {/if}
</div>

<style>
  .card {
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 8px;
    padding: 20px;
  }
  .label {
    font-size: 13px;
    color: #94a3b8;
    margin-bottom: 4px;
  }
  .value {
    font-size: 28px;
    font-weight: 700;
    color: #f1f5f9;
  }
  .subtitle {
    font-size: 12px;
    color: #64748b;
    margin-top: 4px;
  }
</style>
```

- [ ] **Step 3: Create EmptyState.svelte**

```svelte
<script lang="ts">
  let { message = 'No data yet' }: { message?: string } = $props()
</script>

<div class="empty">
  <p>{message}</p>
</div>

<style>
  .empty {
    text-align: center;
    padding: 48px;
    color: #64748b;
    font-size: 15px;
  }
</style>
```

- [ ] **Step 4: Commit**

```bash
git add dashboard/src/lib/components/
git commit -m "feat(dashboard): add Navbar, StatCard, EmptyState components"
```

---

### Task 13: Login page

**Files:**
- Create: `dashboard/src/routes/Login.svelte`

- [ ] **Step 1: Create Login.svelte**

```svelte
<script lang="ts">
  import { login } from '../lib/api'

  let key = $state('')
  let error = $state('')
  let loading = $state(false)

  async function handleSubmit(e: Event) {
    e.preventDefault()
    error = ''
    loading = true
    try {
      const ok = await login(key)
      if (!ok) {
        error = 'Invalid admin key'
      }
    } catch (err) {
      error = 'Connection failed'
    } finally {
      loading = false
    }
  }
</script>

<div class="login-container">
  <div class="login-box">
    <h1>Claudeway</h1>
    <p>Admin Dashboard</p>
    <form onsubmit={handleSubmit}>
      <input
        type="password"
        placeholder="Admin API Key"
        bind:value={key}
        disabled={loading}
      />
      <button type="submit" disabled={loading || !key}>
        {loading ? 'Logging in...' : 'Login'}
      </button>
      {#if error}
        <div class="error">{error}</div>
      {/if}
    </form>
  </div>
</div>

<style>
  .login-container {
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }
  .login-box {
    background: #1e293b;
    border: 1px solid #334155;
    border-radius: 12px;
    padding: 40px;
    width: 360px;
    text-align: center;
  }
  h1 {
    color: #38bdf8;
    margin: 0 0 4px;
  }
  p {
    color: #94a3b8;
    margin: 0 0 24px;
  }
  input {
    width: 100%;
    padding: 10px 12px;
    background: #0f172a;
    border: 1px solid #334155;
    border-radius: 6px;
    color: #e2e8f0;
    font-size: 14px;
    margin-bottom: 12px;
    box-sizing: border-box;
  }
  button {
    width: 100%;
    padding: 10px;
    background: #38bdf8;
    color: #0f172a;
    border: none;
    border-radius: 6px;
    font-weight: 600;
    cursor: pointer;
    font-size: 14px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .error {
    color: #f87171;
    font-size: 13px;
    margin-top: 12px;
  }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/routes/Login.svelte
git commit -m "feat(dashboard): add login page"
```

---

### Task 14: Overview page

**Files:**
- Create: `dashboard/src/routes/Overview.svelte`

- [ ] **Step 1: Create Overview.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getOverview, getCosts } from '../lib/api'
  import StatCard from '../lib/components/StatCard.svelte'
  import Chart from 'chart.js/auto'

  let data: any = $state(null)
  let error = $state('')
  let lineCanvas: HTMLCanvasElement
  let pieCanvas: HTMLCanvasElement
  let lineChart: Chart | null = null
  let pieChart: Chart | null = null

  onMount(async () => {
    try {
      data = await getOverview()
      const costs: any = await getCosts('daily')
      renderLineChart(costs.data)
      renderPieChart(data.models_breakdown)
    } catch (e: any) {
      error = e.message
    }
  })

  function formatUptime(secs: number): string {
    const h = Math.floor(secs / 3600)
    const m = Math.floor((secs % 3600) / 60)
    return `${h}h ${m}m`
  }

  function formatCost(usd: number): string {
    return `$${usd.toFixed(4)}`
  }

  function renderLineChart(costData: any[]) {
    if (!lineCanvas || !costData?.length) return
    if (lineChart) lineChart.destroy()
    // Take last 30 days
    const recent = costData.slice(-30)
    lineChart = new Chart(lineCanvas, {
      type: 'line',
      data: {
        labels: recent.map((d: any) => d.period),
        datasets: [
          {
            label: 'Cost (USD)',
            data: recent.map((d: any) => d.cost_usd),
            borderColor: '#38bdf8',
            tension: 0.3,
          },
          {
            label: 'Requests',
            data: recent.map((d: any) => d.request_count),
            borderColor: '#a78bfa',
            tension: 0.3,
            yAxisID: 'y1',
          },
        ],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' }, position: 'left' },
          y1: { ticks: { color: '#64748b' }, grid: { display: false }, position: 'right' },
        },
      },
    })
  }

  function renderPieChart(breakdown: any[]) {
    if (!pieCanvas || !breakdown?.length) return
    if (pieChart) pieChart.destroy()
    pieChart = new Chart(pieCanvas, {
      type: 'doughnut',
      data: {
        labels: breakdown.map((b: any) => b.model),
        datasets: [{
          data: breakdown.map((b: any) => b.request_count),
          backgroundColor: ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171'],
        }],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
      },
    })
  }
</script>

<h2>Overview</h2>

{#if error}
  <div class="error">{error}</div>
{:else if data}
  <div class="grid">
    <StatCard label="Uptime" value={formatUptime(data.uptime_secs)} />
    <StatCard label="Total Requests" value={data.total_requests} />
    <StatCard label="Active Sessions" value={data.active_sessions} />
    <StatCard label="Total Cost" value={formatCost(data.total_cost_usd)} />
  </div>

  <div class="charts">
    <div class="chart-box">
      <h3>Daily Requests & Cost (last 30 days)</h3>
      <canvas bind:this={lineCanvas}></canvas>
    </div>
    <div class="chart-box small">
      <h3>Model Usage</h3>
      <canvas bind:this={pieCanvas}></canvas>
    </div>
  </div>
{:else}
  <p>Loading...</p>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  h3 { margin: 0 0 12px; font-size: 15px; color: #94a3b8; }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 16px;
    margin-bottom: 24px;
  }
  .charts { display: grid; grid-template-columns: 2fr 1fr; gap: 16px; }
  .chart-box { background: #1e293b; border-radius: 8px; padding: 20px; }
  .error { color: #f87171; padding: 12px; background: #1e293b; border-radius: 8px; }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/routes/Overview.svelte
git commit -m "feat(dashboard): add overview page with stat cards"
```

---

### Task 15: Sessions page

**Files:**
- Create: `dashboard/src/routes/Sessions.svelte`
- Create: `dashboard/src/routes/SessionDetail.svelte`

- [ ] **Step 1: Create Sessions.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getSessions } from '../lib/api'
  import { link } from 'svelte-spa-router'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let sessions: any[] = $state([])
  let total = $state(0)
  let page = $state(1)
  let loading = $state(true)

  onMount(() => load())

  async function load() {
    loading = true
    try {
      const res: any = await getSessions(page)
      sessions = res.sessions
      total = res.total
    } catch {}
    loading = false
  }

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleString()
  }
</script>

<h2>Sessions</h2>

{#if loading}
  <p>Loading...</p>
{:else if sessions.length === 0}
  <EmptyState message="No sessions yet" />
{:else}
  <table>
    <thead>
      <tr>
        <th>Session ID</th>
        <th>Model</th>
        <th>Tasks</th>
        <th>Cost</th>
        <th>Created</th>
      </tr>
    </thead>
    <tbody>
      {#each sessions as s}
        <tr>
          <td><a href="/sessions/{s.session_id}" use:link>{s.session_id.slice(0, 8)}...</a></td>
          <td>{s.model || '—'}</td>
          <td>{s.task_count}</td>
          <td>${s.cost_usd.toFixed(4)}</td>
          <td>{formatDate(s.created_at)}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <div class="pagination">
    <button onclick={() => { page--; load() }} disabled={page <= 1}>Prev</button>
    <span>Page {page}</span>
    <button onclick={() => { page++; load() }} disabled={sessions.length < 20}>Next</button>
  </div>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
  td a { color: #38bdf8; text-decoration: none; }
  td a:hover { text-decoration: underline; }
  .pagination { display: flex; align-items: center; gap: 12px; margin-top: 16px; }
  .pagination button {
    padding: 6px 12px; background: #1e293b; border: 1px solid #334155;
    color: #e2e8f0; border-radius: 4px; cursor: pointer;
  }
  .pagination button:disabled { opacity: 0.4; cursor: not-allowed; }
</style>
```

- [ ] **Step 2: Create SessionDetail.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getSessionDetail } from '../lib/api'
  import StatCard from '../lib/components/StatCard.svelte'

  export let params: { id: string } = { id: '' }

  let session: any = $state(null)

  onMount(async () => {
    try {
      session = await getSessionDetail(params.id)
    } catch {}
  })
</script>

<h2>Session Detail</h2>

{#if session}
  <div class="grid">
    <StatCard label="Tasks" value={session.task_count} />
    <StatCard label="Cost" value={'$' + session.cost_usd.toFixed(4)} />
    <StatCard label="Input Tokens" value={session.tokens.input} />
    <StatCard label="Output Tokens" value={session.tokens.output} />
  </div>

  <div class="meta">
    <p><strong>ID:</strong> {session.session_id}</p>
    <p><strong>Model:</strong> {session.model || '—'}</p>
    <p><strong>Key:</strong> {session.key_id}</p>
    <p><strong>Workdir:</strong> {session.workdir}</p>
    <p><strong>Created:</strong> {new Date(session.created_at).toLocaleString()}</p>
    <p><strong>Last Used:</strong> {new Date(session.last_used).toLocaleString()}</p>
  </div>
{:else}
  <p>Loading...</p>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin-bottom: 24px; }
  .meta { background: #1e293b; border: 1px solid #334155; border-radius: 8px; padding: 20px; }
  .meta p { margin: 8px 0; font-size: 14px; }
  .meta strong { color: #94a3b8; }
</style>
```

- [ ] **Step 3: Commit**

```bash
git add dashboard/src/routes/Sessions.svelte dashboard/src/routes/SessionDetail.svelte
git commit -m "feat(dashboard): add sessions list and detail pages"
```

---

### Task 16: Logs page

**Files:**
- Create: `dashboard/src/routes/Logs.svelte`

- [ ] **Step 1: Create Logs.svelte**

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { getLogs } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let entries: any[] = $state([])
  let loading = $state(true)
  let keyFilter = $state('')
  let pollInterval: ReturnType<typeof setInterval>

  onMount(async () => {
    await load()
    pollInterval = setInterval(poll, 5000)
  })

  onDestroy(() => {
    if (pollInterval) clearInterval(pollInterval)
  })

  async function load() {
    loading = true
    try {
      const res: any = await getLogs({ key_id: keyFilter || undefined, limit: 100 })
      entries = res.entries
    } catch {}
    loading = false
  }

  async function poll() {
    if (entries.length === 0) return
    const lastTs = entries[0]?.timestamp
    if (!lastTs) return
    try {
      const res: any = await getLogs({ after: lastTs, key_id: keyFilter || undefined })
      if (res.entries.length > 0) {
        entries = [...res.entries, ...entries].slice(0, 200)
      }
    } catch {}
  }

  function levelColor(level: string): string {
    switch (level?.toUpperCase()) {
      case 'ERROR': return '#f87171'
      case 'WARN': return '#fbbf24'
      default: return '#4ade80'
    }
  }
</script>

<h2>Logs</h2>

<div class="filters">
  <input placeholder="Filter by key ID" bind:value={keyFilter} onchange={load} />
</div>

{#if loading}
  <p>Loading...</p>
{:else if entries.length === 0}
  <EmptyState message="No log entries" />
{:else}
  <div class="log-list">
    {#each entries as entry}
      <div class="log-entry">
        <span class="level" style="color: {levelColor(entry.level)}">{entry.level}</span>
        <span class="timestamp">{entry.timestamp}</span>
        <span class="key">{entry.key_id || '—'}</span>
        <span class="message">{entry.message}</span>
        {#if entry.cost_usd}
          <span class="cost">${entry.cost_usd.toFixed(4)}</span>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  .filters { margin-bottom: 16px; }
  .filters input {
    padding: 8px 12px; background: #1e293b; border: 1px solid #334155;
    border-radius: 6px; color: #e2e8f0; font-size: 13px; width: 240px;
  }
  .log-list { display: flex; flex-direction: column; gap: 2px; }
  .log-entry {
    display: flex; gap: 12px; padding: 8px 12px; background: #1e293b;
    border-radius: 4px; font-size: 13px; font-family: monospace;
  }
  .level { font-weight: 600; min-width: 48px; }
  .timestamp { color: #64748b; min-width: 200px; }
  .key { color: #94a3b8; min-width: 80px; }
  .message { flex: 1; color: #cbd5e1; }
  .cost { color: #fbbf24; }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/routes/Logs.svelte
git commit -m "feat(dashboard): add log viewer with polling"
```

---

### Task 17: Keys page

**Files:**
- Create: `dashboard/src/routes/Keys.svelte`

- [ ] **Step 1: Create Keys.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getKeys } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'

  let keys: any[] = $state([])
  let loading = $state(true)

  onMount(async () => {
    try {
      const res: any = await getKeys()
      keys = res.keys
    } catch {}
    loading = false
  })
</script>

<h2>API Keys</h2>

{#if loading}
  <p>Loading...</p>
{:else if keys.length === 0}
  <EmptyState message="No API keys configured" />
{:else}
  <table>
    <thead>
      <tr>
        <th>Key ID</th>
        <th>Total Requests</th>
        <th>Total Cost</th>
      </tr>
    </thead>
    <tbody>
      {#each keys as k}
        <tr>
          <td>{k.key_id}</td>
          <td>{k.total_requests}</td>
          <td>${k.total_cost_usd.toFixed(4)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/routes/Keys.svelte
git commit -m "feat(dashboard): add API keys page"
```

---

### Task 18: Cost Analytics page

**Files:**
- Create: `dashboard/src/routes/Costs.svelte`

- [ ] **Step 1: Create Costs.svelte**

```svelte
<script lang="ts">
  import { onMount } from 'svelte'
  import { getCosts } from '../lib/api'
  import EmptyState from '../lib/components/EmptyState.svelte'
  import Chart from 'chart.js/auto'

  let data: any = $state(null)
  let groupBy = $state('daily')
  let loading = $state(true)
  let chartCanvas: HTMLCanvasElement
  let chartInstance: Chart | null = null

  onMount(() => load())

  async function load() {
    loading = true
    try {
      const res: any = await getCosts(groupBy)
      data = res.data
      renderChart()
    } catch {}
    loading = false
  }

  let modelCanvas: HTMLCanvasElement
  let keyCanvas: HTMLCanvasElement
  let modelChart: Chart | null = null
  let keyChart: Chart | null = null

  function renderChart() {
    if (!chartCanvas || !data || data.length === 0) return
    if (chartInstance) chartInstance.destroy()

    chartInstance = new Chart(chartCanvas, {
      type: 'bar',
      data: {
        labels: data.map((d: any) => d.period),
        datasets: [{
          label: 'Cost (USD)',
          data: data.map((d: any) => d.cost_usd),
          backgroundColor: '#38bdf8',
          borderRadius: 4,
        }],
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })

    renderModelChart()
    renderKeyChart()
  }

  function renderModelChart() {
    if (!modelCanvas || !data || data.length === 0) return
    if (modelChart) modelChart.destroy()

    // Collect all unique models
    const models = new Set<string>()
    data.forEach((d: any) => d.by_model?.forEach((m: any) => models.add(m.model)))
    const modelList = [...models]
    const colors = ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171']

    modelChart = new Chart(modelCanvas, {
      type: 'bar',
      data: {
        labels: data.map((d: any) => d.period),
        datasets: modelList.map((model, i) => ({
          label: model,
          data: data.map((d: any) => {
            const m = d.by_model?.find((b: any) => b.model === model)
            return m ? m.cost_usd : 0
          }),
          backgroundColor: colors[i % colors.length],
        })),
      },
      options: {
        responsive: true,
        plugins: { legend: { labels: { color: '#94a3b8' } } },
        scales: {
          x: { stacked: true, ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { stacked: true, ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })
  }

  function renderKeyChart() {
    if (!keyCanvas || !data || data.length === 0) return
    if (keyChart) keyChart.destroy()

    // Aggregate cost per key across all periods
    const keyTotals: Record<string, number> = {}
    data.forEach((d: any) => d.by_key?.forEach((k: any) => {
      keyTotals[k.key_id] = (keyTotals[k.key_id] || 0) + k.cost_usd
    }))

    const keys = Object.keys(keyTotals)
    const colors = ['#38bdf8', '#a78bfa', '#4ade80', '#fbbf24', '#f87171']

    keyChart = new Chart(keyCanvas, {
      type: 'bar',
      data: {
        labels: keys,
        datasets: [{
          label: 'Total Cost (USD)',
          data: keys.map(k => keyTotals[k]),
          backgroundColor: keys.map((_, i) => colors[i % colors.length]),
          borderRadius: 4,
        }],
      },
      options: {
        responsive: true,
        indexAxis: 'y',
        plugins: { legend: { display: false } },
        scales: {
          x: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
          y: { ticks: { color: '#64748b' }, grid: { color: '#1e293b' } },
        },
      },
    })
  }
</script>

<h2>Cost Analytics</h2>

<div class="toggle">
  {#each ['daily', 'weekly', 'monthly'] as g}
    <button class:active={groupBy === g} onclick={() => { groupBy = g; load() }}>{g}</button>
  {/each}
</div>

{#if loading}
  <p>Loading...</p>
{:else if !data || data.length === 0}
  <EmptyState message="No cost data yet" />
{:else}
  <h3>Total Cost by Period</h3>
  <div class="chart-container">
    <canvas bind:this={chartCanvas}></canvas>
  </div>

  <h3>Cost by Model (stacked)</h3>
  <div class="chart-container">
    <canvas bind:this={modelCanvas}></canvas>
  </div>

  <h3>Cost by Key</h3>
  <div class="chart-container">
    <canvas bind:this={keyCanvas}></canvas>
  </div>

  <table>
    <thead>
      <tr>
        <th>Period</th>
        <th>Requests</th>
        <th>Cost</th>
      </tr>
    </thead>
    <tbody>
      {#each data as d}
        <tr>
          <td>{d.period}</td>
          <td>{d.request_count}</td>
          <td>${d.cost_usd.toFixed(4)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  h2 { margin: 0 0 20px; font-size: 22px; }
  h3 { margin: 20px 0 8px; font-size: 15px; color: #94a3b8; }
  .toggle { display: flex; gap: 8px; margin-bottom: 20px; }
  .toggle button {
    padding: 6px 16px; background: #1e293b; border: 1px solid #334155;
    color: #94a3b8; border-radius: 4px; cursor: pointer; text-transform: capitalize;
  }
  .toggle button.active { background: #38bdf8; color: #0f172a; border-color: #38bdf8; }
  .chart-container { background: #1e293b; border-radius: 8px; padding: 20px; margin-bottom: 20px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 10px 12px; text-align: left; border-bottom: 1px solid #334155; }
  th { color: #94a3b8; font-size: 13px; font-weight: 500; }
</style>
```

- [ ] **Step 2: Commit**

```bash
git add dashboard/src/routes/Costs.svelte
git commit -m "feat(dashboard): add cost analytics page with Chart.js"
```

---

## Chunk 4: Integration and Final Verification

### Task 19: Update .gitignore and final wiring

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Add dashboard build output to .gitignore**

Add to `.gitignore`:

```
dashboard/dist/
dashboard/node_modules/
```

- [ ] **Step 2: Full build test**

```bash
cd dashboard && npm install && npm run build && cd ..
cargo build --features dashboard
```

Expected: Both Svelte and Rust build successfully

- [ ] **Step 3: Build without dashboard feature**

```bash
cargo build
```

Expected: Compiles without errors — all dashboard code gated behind `#[cfg(feature = "dashboard")]`

- [ ] **Step 4: Run all Rust tests**

```bash
cargo test -- --nocapture
```

Expected: ALL PASS

- [ ] **Step 5: Commit**

```bash
git add .gitignore
git commit -m "chore: add dashboard build artifacts to gitignore"
```

---

### Task 20: Manual smoke test

- [ ] **Step 1: Start server with dashboard**

```bash
cargo run --features dashboard
```

- [ ] **Step 2: Verify dashboard loads**

Open `http://localhost:3000/dashboard` in browser.
Expected: Login page renders.

- [ ] **Step 3: Test login**

Enter the admin key printed at startup.
Expected: Redirects to overview page.

- [ ] **Step 4: Verify each page**

Navigate to Sessions, Logs, Keys, Costs.
Expected: Each page loads without errors (may show empty states).

- [ ] **Step 5: Verify non-dashboard build still works**

```bash
cargo run
```

Expected: Server starts normally, no `/dashboard` route.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "feat: admin dashboard v1 complete"
```
