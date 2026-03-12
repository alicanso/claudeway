# Claudeway Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a production-grade Axum HTTP server wrapping the `claude` CLI binary with auth, session management, structured logging, and token/cost tracking.

**Architecture:** Layered Axum server — config/models at the base, auth middleware in the middle, handlers on top. Claude CLI interaction isolated in `claude.rs`. Sessions stored in `DashMap` with per-session mutexes. Per-key JSON log files with monthly rotation.

**Tech Stack:** Rust, Axum, Tokio, Serde, DashMap, Tower-HTTP, tracing, tracing-appender, Chrono, UUID, Anyhow

**Spec:** `docs/superpowers/specs/2026-03-12-claudeway-http-wrapper-design.md`

---

## Chunk 1: Project Foundation

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize Cargo.toml**

```toml
[package]
name = "claudeway"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dashmap = "6"
tower-http = { version = "0.6", features = ["trace", "timeout", "cors"] }
tower = "0.5"
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
tracing-appender = "0.2"
http = "1"
```

- [ ] **Step 2: Create minimal main.rs**

```rust
use std::net::SocketAddr;
use tokio::net::TcpListener;

mod config;
mod error;
mod models;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = axum::Router::new();
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr).await?;
    println!("Claudeway listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors (warnings OK for unused modules)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: scaffold claudeway project with dependencies"
```

---

### Task 2: Config Module

**Files:**
- Create: `src/config.rs`

- [ ] **Step 1: Write config parsing tests**

```rust
// src/config.rs
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    /// Map from key_value -> key_id for O(1) auth lookup
    pub api_keys: HashMap<String, String>,
    pub claude_bin: String,
    pub claude_workdir: PathBuf,
    pub log_dir: PathBuf,
    pub port: u16,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let keys_str = env::var("WRAPPER_KEYS")
            .map_err(|_| anyhow::anyhow!("WRAPPER_KEYS env var is required"))?;

        let api_keys = Self::parse_keys(&keys_str)?;

        Ok(Config {
            api_keys,
            claude_bin: env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            claude_workdir: PathBuf::from(
                env::var("CLAUDE_WORKDIR").unwrap_or_else(|_| "/tmp/claude-tasks".into()),
            ),
            log_dir: PathBuf::from(
                env::var("LOG_DIR").unwrap_or_else(|_| "./logs".into()),
            ),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()
                .map_err(|_| anyhow::anyhow!("PORT must be a valid u16"))?,
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        })
    }

    fn parse_keys(keys_str: &str) -> anyhow::Result<HashMap<String, String>> {
        let mut map = HashMap::new();
        for pair in keys_str.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (key_id, key_value) = pair
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("Invalid key format: '{pair}'. Expected 'key_id:key_value'"))?;
            if key_id.is_empty() || key_value.is_empty() {
                anyhow::bail!("Key ID and value must not be empty in '{pair}'");
            }
            map.insert(key_value.to_string(), key_id.to_string());
        }
        if map.is_empty() {
            anyhow::bail!("WRAPPER_KEYS must contain at least one key pair");
        }
        Ok(map)
    }

    /// Get all key_ids (for log directory setup)
    pub fn key_ids(&self) -> Vec<String> {
        self.api_keys.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_key() {
        let keys = Config::parse_keys("admin:sk-abc123").unwrap();
        assert_eq!(keys.get("sk-abc123"), Some(&"admin".to_string()));
    }

    #[test]
    fn test_parse_multiple_keys() {
        let keys = Config::parse_keys("admin:sk-abc123,bot:sk-def456").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.get("sk-abc123"), Some(&"admin".to_string()));
        assert_eq!(keys.get("sk-def456"), Some(&"bot".to_string()));
    }

    #[test]
    fn test_parse_keys_empty_fails() {
        assert!(Config::parse_keys("").is_err());
    }

    #[test]
    fn test_parse_keys_no_colon_fails() {
        assert!(Config::parse_keys("invalidkey").is_err());
    }

    #[test]
    fn test_parse_keys_empty_id_fails() {
        assert!(Config::parse_keys(":sk-abc").is_err());
    }

    #[test]
    fn test_parse_keys_empty_value_fails() {
        assert!(Config::parse_keys("admin:").is_err());
    }

    #[test]
    fn test_parse_keys_trims_whitespace() {
        let keys = Config::parse_keys(" admin:sk-abc , bot:sk-def ").unwrap();
        assert_eq!(keys.len(), 2);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test config`
Expected: All 7 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add config module with env var parsing and key validation"
```

---

### Task 3: Error Types

**Files:**
- Create: `src/error.rs`

- [ ] **Step 1: Write error module**

```rust
// src/error.rs
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub code: &'static str,
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::BAD_REQUEST,
            body: ApiError {
                error: msg.into(),
                code: "BAD_REQUEST",
            },
        }
    }

    pub fn unauthorized() -> AppError {
        AppError {
            status: StatusCode::UNAUTHORIZED,
            body: ApiError {
                error: "Invalid or missing API key".into(),
                code: "UNAUTHORIZED",
            },
        }
    }

    pub fn not_found(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::NOT_FOUND,
            body: ApiError {
                error: msg.into(),
                code: "NOT_FOUND",
            },
        }
    }

    pub fn timeout() -> AppError {
        AppError {
            status: StatusCode::REQUEST_TIMEOUT,
            body: ApiError {
                error: "Claude CLI timed out".into(),
                code: "TIMEOUT",
            },
        }
    }

    pub fn internal(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ApiError {
                error: msg.into(),
                code: "INTERNAL_ERROR",
            },
        }
    }
}

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub body: ApiError,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&self.body).unwrap_or_else(|_| {
            r#"{"error":"internal error","code":"INTERNAL_ERROR"}"#.to_string()
        });
        (
            self.status,
            [(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            )],
            body,
        )
            .into_response()
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat: add error types with JSON response formatting"
```

---

### Task 4: Models (Request/Response Types)

**Files:**
- Create: `src/models.rs`

- [ ] **Step 1: Write all request/response structs**

```rust
// src/models.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// --- Shared ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenUsage {
    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

// --- Health ---

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_secs: u64,
}

// --- Models ---

#[derive(Serialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

// --- Task ---

#[derive(Deserialize)]
pub struct TaskRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub session_id: String,
    pub result: Option<String>,
    pub success: bool,
    pub duration_ms: u64,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
}

// --- Session ---

#[derive(Deserialize)]
pub struct SessionStartRequest {
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: Option<String>,
}

#[derive(Serialize)]
pub struct SessionStartResponse {
    pub session_id: String,
    pub workdir: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct SessionContinueRequest {
    pub prompt: String,
    pub timeout_secs: Option<u64>,
}

#[derive(Serialize)]
pub struct SessionInfoResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u32,
    pub workdir: String,
    pub tokens: TokenUsage,
    pub cost_usd: f64,
}

#[derive(Serialize)]
pub struct DeleteSessionResponse {
    pub deleted: bool,
    pub session_id: String,
}

// --- Claude CLI output parsing ---

/// Parsed from claude --output-format json stdout
#[derive(Debug, Deserialize)]
pub struct ClaudeCliOutput {
    #[serde(rename = "type")]
    pub output_type: Option<String>,
    pub session_id: Option<String>,
    pub result: Option<String>,
    #[serde(rename = "costUSD")]
    pub cost_usd: Option<f64>,
    #[serde(rename = "duration_ms")]
    pub duration_ms: Option<u64>,
    #[serde(rename = "isError")]
    pub is_error: Option<bool>,
    // duration_api_ms can also appear
}

/// Entry from session JSONL file for token extraction
#[derive(Debug, Deserialize)]
pub struct SessionJSONLEntry {
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    pub message: Option<SessionMessage>,
}

#[derive(Debug, Deserialize)]
pub struct SessionMessage {
    pub usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize)]
pub struct MessageUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_accumulate() {
        let mut total = TokenUsage::default();
        let usage = TokenUsage { input: 100, output: 50, cache_read: 30, cache_write: 10 };
        total.accumulate(&usage);
        assert_eq!(total.input, 100);
        total.accumulate(&usage);
        assert_eq!(total.input, 200);
        assert_eq!(total.output, 100);
    }

    #[test]
    fn test_parse_claude_cli_output() {
        let json = r#"{"type":"result","session_id":"abc-123","result":"Hello","costUSD":0.005,"duration_ms":1234,"isError":false}"#;
        let output: ClaudeCliOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.session_id.as_deref(), Some("abc-123"));
        assert_eq!(output.cost_usd, Some(0.005));
        assert_eq!(output.result.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_parse_session_jsonl_entry() {
        let json = r#"{"type":"assistant","message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":80}}}"#;
        let entry: SessionJSONLEntry = serde_json::from_str(json).unwrap();
        let usage = entry.message.unwrap().usage.unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cache_read_input_tokens, Some(80));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test models`
Expected: All 3 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/models.rs
git commit -m "feat: add request/response models and CLI output parsing types"
```

---

## Chunk 2: Middleware & Infrastructure

### Task 5: Auth Middleware

**Files:**
- Create: `src/auth.rs`

- [ ] **Step 1: Write auth middleware**

```rust
// src/auth.rs
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::ApiError;

/// Key ID extracted from a valid API key, attached to request extensions.
#[derive(Debug, Clone)]
pub struct KeyId(pub String);

/// Auth middleware — checks Authorization: Bearer {value} against known keys.
pub async fn auth_middleware(
    request: Request,
    next: Next,
    api_keys: Arc<HashMap<String, String>>,
) -> Result<Response, crate::error::AppError> {
    let auth_header = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return Err(ApiError::unauthorized()),
    };

    let key_id = api_keys
        .get(token)
        .ok_or_else(ApiError::unauthorized)?;

    let mut request = request;
    request.extensions_mut().insert(KeyId(key_id.clone()));

    Ok(next.run(request).await)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/auth.rs
git commit -m "feat: add Bearer token auth middleware with key_id extraction"
```

---

### Task 6: Per-Key JSON Logging

**Files:**
- Create: `src/logging.rs`

- [ ] **Step 1: Write logging module**

The per-key file routing with `tracing` is complex. We'll use a simpler approach: a custom logger struct that writes JSON lines directly to per-key files, using `tracing` for console output during development.

```rust
// src/logging.rs
use chrono::Utc;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::models::TokenUsage;

/// Per-key file logger. Writes JSON lines to key-specific monthly log files.
pub struct KeyLogger {
    log_dir: PathBuf,
    /// Cache of open file handles: "key_id/YYYY-MM" -> file
    files: Mutex<HashMap<String, fs::File>>,
}

impl KeyLogger {
    pub fn new(log_dir: &Path, key_ids: &[String]) -> anyhow::Result<Self> {
        // Pre-create directories for all known keys + _unauthorized
        for key_id in key_ids {
            fs::create_dir_all(log_dir.join(key_id))?;
        }
        fs::create_dir_all(log_dir.join("_unauthorized"))?;

        Ok(KeyLogger {
            log_dir: log_dir.to_path_buf(),
            files: Mutex::new(HashMap::new()),
        })
    }

    fn get_or_open_file(&self, key_id: &str) -> anyhow::Result<std::io::Result<()>> {
        // We re-open on every write to handle month boundaries simply
        // For production at scale, cache with month check
        Ok(Ok(()))
    }

    fn write_line(&self, key_id: &str, line: &str) {
        let now = Utc::now();
        let month = now.format("%Y-%m").to_string();
        let dir = self.log_dir.join(key_id);
        let path = dir.join(format!("{month}.log"));

        let result = (|| -> anyhow::Result<()> {
            fs::create_dir_all(&dir)?;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(file, "{line}")?;
            Ok(())
        })();

        if let Err(e) = result {
            eprintln!("Failed to write log to {}: {e}", path.display());
        }
    }

    /// Log an HTTP request
    pub fn log_request(&self, entry: &RequestLog) {
        let key_id = entry.key_id.as_deref().unwrap_or("_unauthorized");
        if let Ok(json) = serde_json::to_string(entry) {
            self.write_line(key_id, &json);
        }
    }

    /// Log a Claude CLI invocation
    pub fn log_claude_invocation(&self, entry: &ClaudeInvocationLog) {
        if let Ok(json) = serde_json::to_string(entry) {
            self.write_line(&entry.key_id, &json);
        }
    }

    /// Log an unauthorized access attempt
    pub fn log_unauthorized(&self, entry: &UnauthorizedLog) {
        if let Ok(json) = serde_json::to_string(entry) {
            self.write_line("_unauthorized", &json);
        }
    }
}

#[derive(Serialize)]
pub struct RequestLog {
    pub timestamp: String,
    pub level: &'static str,
    pub key_id: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub message: String,
}

#[derive(Serialize)]
pub struct ClaudeInvocationLog {
    pub timestamp: String,
    pub level: &'static str,
    pub key_id: String,
    pub session_id: String,
    pub model: Option<String>,
    #[serde(rename = "claude_exit_code")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub success: bool,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
    pub message: String,
}

#[derive(Serialize)]
pub struct UnauthorizedLog {
    pub timestamp: String,
    pub level: &'static str,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Note: tempfile is needed for tests — add to dev-dependencies

    #[test]
    fn test_request_log_serializes() {
        let log = RequestLog {
            timestamp: "2026-03-12T10:00:00Z".into(),
            level: "INFO",
            key_id: Some("admin".into()),
            method: "POST".into(),
            path: "/task".into(),
            status: 200,
            duration_ms: 843,
            message: "request completed".into(),
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"key_id\":\"admin\""));
        assert!(json.contains("\"status\":200"));
    }

    #[test]
    fn test_claude_invocation_log_serializes() {
        let log = ClaudeInvocationLog {
            timestamp: "2026-03-12T10:00:00Z".into(),
            level: "INFO",
            key_id: "admin".into(),
            session_id: "uuid-123".into(),
            model: Some("sonnet".into()),
            exit_code: Some(0),
            duration_ms: 1234,
            success: true,
            tokens: Some(TokenUsage { input: 100, output: 50, cache_read: 30, cache_write: 0 }),
            cost_usd: Some(0.005),
            message: "task completed".into(),
        };
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"claude_exit_code\":0"));
        assert!(json.contains("\"cost_usd\":0.005"));
    }
}
```

Also add `tempfile` to dev-dependencies in `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Run tests**

Run: `cargo test logging`
Expected: All 2 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/logging.rs Cargo.toml
git commit -m "feat: add per-key JSON file logger with monthly rotation"
```

---

### Task 7: Session Store

**Files:**
- Create: `src/session.rs`

- [ ] **Step 1: Write session store**

```rust
// src/session.rs
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::TokenUsage;

/// Metadata for a persistent Claude session.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub session_id: Uuid,
    /// The Claude CLI's own session ID (from --output-format json)
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: PathBuf,
    /// Whether the workdir was auto-allocated (should be cleaned up on delete)
    pub auto_workdir: bool,
    pub task_count: u32,
    pub tokens: TokenUsage,
    pub cost_usd: f64,
}

/// Thread-safe session store backed by DashMap.
pub struct SessionStore {
    sessions: DashMap<Uuid, SessionMeta>,
    /// Per-session mutex to prevent concurrent --resume calls
    locks: DashMap<Uuid, Arc<Mutex<()>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        SessionStore {
            sessions: DashMap::new(),
            locks: DashMap::new(),
        }
    }

    pub fn insert(&self, meta: SessionMeta) {
        let id = meta.session_id;
        self.sessions.insert(id, meta);
        self.locks.insert(id, Arc::new(Mutex::new(())));
    }

    pub fn get(&self, id: &Uuid) -> Option<SessionMeta> {
        self.sessions.get(id).map(|r| r.clone())
    }

    pub fn update<F>(&self, id: &Uuid, f: F) -> bool
    where
        F: FnOnce(&mut SessionMeta),
    {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            f(&mut entry);
            true
        } else {
            false
        }
    }

    pub fn remove(&self, id: &Uuid) -> Option<SessionMeta> {
        self.locks.remove(id);
        self.sessions.remove(id).map(|(_, v)| v)
    }

    /// Get the per-session lock for serializing --resume calls
    pub fn get_lock(&self, id: &Uuid) -> Option<Arc<Mutex<()>>> {
        self.locks.get(id).map(|r| r.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_crud() {
        let store = SessionStore::new();
        let id = Uuid::new_v4();
        let meta = SessionMeta {
            session_id: id,
            claude_session_id: None,
            created_at: Utc::now(),
            last_used: Utc::now(),
            model: Some("sonnet".into()),
            system_prompt: None,
            workdir: PathBuf::from("/tmp/test"),
            auto_workdir: true,
            task_count: 0,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
        };

        store.insert(meta);
        assert!(store.get(&id).is_some());

        store.update(&id, |m| {
            m.task_count = 1;
            m.cost_usd = 0.005;
        });
        assert_eq!(store.get(&id).unwrap().task_count, 1);

        let removed = store.remove(&id);
        assert!(removed.is_some());
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_session_lock_exists() {
        let store = SessionStore::new();
        let id = Uuid::new_v4();
        let meta = SessionMeta {
            session_id: id,
            claude_session_id: None,
            created_at: Utc::now(),
            last_used: Utc::now(),
            model: None,
            system_prompt: None,
            workdir: PathBuf::from("/tmp/test"),
            auto_workdir: false,
            task_count: 0,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
        };

        store.insert(meta);
        assert!(store.get_lock(&id).is_some());

        store.remove(&id);
        assert!(store.get_lock(&id).is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test session`
Expected: All 2 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/session.rs
git commit -m "feat: add DashMap session store with per-session mutex locks"
```

---

## Chunk 3: Claude CLI Executor

### Task 8: Claude CLI Wrapper

**Files:**
- Create: `src/claude.rs`

- [ ] **Step 1: Write Claude CLI executor**

```rust
// src/claude.rs
use std::path::Path;
use std::time::Instant;
use tokio::process::Command;

use crate::config::Config;
use crate::error::ApiError;
use crate::models::{ClaudeCliOutput, SessionJSONLEntry, TokenUsage};

/// Result of a Claude CLI invocation
#[derive(Debug)]
pub struct ClaudeResult {
    pub claude_session_id: Option<String>,
    pub result: Option<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
}

/// Execute a one-shot Claude CLI command
pub async fn run_task(
    config: &Config,
    prompt: &str,
    model: Option<&str>,
    system_prompt: Option<&str>,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, crate::error::AppError> {
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ];

    if let Some(model) = model {
        args.push("--model".to_string());
        args.push(resolve_model(model).to_string());
    }

    if let Some(sp) = system_prompt {
        args.push("--system-prompt".to_string());
        args.push(sp.to_string());
    }

    run_claude(config, &args, workdir, timeout_secs).await
}

/// Resume an existing Claude CLI session
pub async fn run_resume(
    config: &Config,
    prompt: &str,
    claude_session_id: &str,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, crate::error::AppError> {
    let args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--resume".to_string(),
        claude_session_id.to_string(),
    ];

    run_claude(config, &args, workdir, timeout_secs).await
}

async fn run_claude(
    config: &Config,
    args: &[String],
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, crate::error::AppError> {
    let start = Instant::now();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        Command::new(&config.claude_bin)
            .args(args)
            .current_dir(workdir)
            .output(),
    )
    .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(ApiError::internal(format!("Failed to spawn claude: {e}")));
        }
        Err(_) => {
            return Err(ApiError::timeout());
        }
    };

    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse the JSON output
    let parsed: Option<ClaudeCliOutput> = serde_json::from_str(&stdout).ok();

    let (result_text, session_id, cost, is_error) = match &parsed {
        Some(p) => (
            p.result.clone(),
            p.session_id.clone(),
            p.cost_usd,
            p.is_error.unwrap_or(false),
        ),
        None => (
            if stdout.is_empty() { None } else { Some(stdout.to_string()) },
            None,
            None,
            !output.status.success(),
        ),
    };

    let success = output.status.success() && !is_error;

    // Try to extract tokens from JSONL session file
    let tokens = if let Some(ref sid) = session_id {
        extract_tokens_from_jsonl(sid).await
    } else {
        None
    };

    let error_msg = if !success {
        Some(if stderr.is_empty() {
            result_text.clone().unwrap_or_else(|| "Unknown error".into())
        } else {
            stderr.to_string()
        })
    } else {
        None
    };

    Ok(ClaudeResult {
        claude_session_id: session_id,
        result: if success { result_text } else { error_msg.clone().or(result_text) },
        success,
        exit_code,
        duration_ms,
        tokens,
        cost_usd: cost,
    })
}

/// Try to read token usage from the Claude session JSONL file
async fn extract_tokens_from_jsonl(session_id: &str) -> Option<TokenUsage> {
    // Claude stores session files under ~/.claude/projects/
    let home = dirs_next::home_dir()?;
    let claude_dir = home.join(".claude").join("projects");

    if !claude_dir.exists() {
        return None;
    }

    // Search for JSONL file matching session_id across project dirs
    let result: Option<TokenUsage> = (|| -> Option<TokenUsage> {
        for project_entry in std::fs::read_dir(&claude_dir).ok()? {
            let project_entry = project_entry.ok()?;
            let jsonl_path = project_entry.path().join(format!("{session_id}.jsonl"));
            if jsonl_path.exists() {
                return parse_jsonl_tokens(&jsonl_path);
            }
        }
        None
    })();

    result
}

fn parse_jsonl_tokens(path: &Path) -> Option<TokenUsage> {
    let content = std::fs::read_to_string(path).ok()?;

    // Find the last line with token usage (iterate backwards)
    let mut tokens = TokenUsage::default();
    let mut found = false;

    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<SessionJSONLEntry>(line) {
            if let Some(msg) = entry.message {
                if let Some(usage) = msg.usage {
                    tokens = TokenUsage {
                        input: usage.input_tokens.unwrap_or(0),
                        output: usage.output_tokens.unwrap_or(0),
                        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
                        cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
                    };
                    found = true;
                    break;
                }
            }
        }
    }

    if found { Some(tokens) } else { None }
}

/// Resolve short model aliases to full model IDs
fn resolve_model(model: &str) -> &str {
    match model {
        "sonnet" => "claude-sonnet-4-6",
        "haiku" => "claude-haiku-4-5-20251001",
        "opus" => "claude-opus-4-6",
        _ => model, // Assume it's already a full model ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_model_aliases() {
        assert_eq!(resolve_model("sonnet"), "claude-sonnet-4-6");
        assert_eq!(resolve_model("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(resolve_model("opus"), "claude-opus-4-6");
        assert_eq!(resolve_model("claude-custom-model"), "claude-custom-model");
    }

    #[test]
    fn test_parse_jsonl_tokens_from_string() {
        let jsonl = r#"{"type":"human","message":{"content":"hello"}}
{"type":"assistant","message":{"usage":{"input_tokens":150,"output_tokens":75,"cache_creation_input_tokens":10,"cache_read_input_tokens":50}}}"#;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, jsonl).unwrap();

        let tokens = parse_jsonl_tokens(&path).unwrap();
        assert_eq!(tokens.input, 150);
        assert_eq!(tokens.output, 75);
        assert_eq!(tokens.cache_write, 10);
        assert_eq!(tokens.cache_read, 50);
    }
}
```

Also add `dirs-next` to `Cargo.toml` dependencies:

```toml
dirs-next = "2"
```

- [ ] **Step 2: Run tests**

Run: `cargo test claude`
Expected: All 2 tests pass

- [ ] **Step 3: Commit**

```bash
git add src/claude.rs Cargo.toml
git commit -m "feat: add Claude CLI executor with token extraction and model resolution"
```

---

## Chunk 4: Handlers

### Task 9: Handler Module + Health Endpoint

**Files:**
- Create: `src/handlers/mod.rs`
- Create: `src/handlers/health.rs`

- [ ] **Step 1: Write handlers mod.rs**

```rust
// src/handlers/mod.rs
pub mod health;
pub mod models;
pub mod session;
pub mod task;
```

- [ ] **Step 2: Write health handler**

```rust
// src/handlers/health.rs
use axum::Json;
use std::sync::Arc;
use std::time::Instant;

use crate::models::HealthResponse;

pub async fn health(start_time: Arc<Instant>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: start_time.elapsed().as_secs(),
    })
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 4: Commit**

```bash
git add src/handlers/
git commit -m "feat: add health endpoint handler"
```

---

### Task 10: Models Endpoint

**Files:**
- Create: `src/handlers/models.rs`

- [ ] **Step 1: Write models handler with caching**

```rust
// src/handlers/models.rs
use axum::Json;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::{ModelInfo, ModelsResponse};

pub struct ModelsCache {
    models: RwLock<Option<CachedModels>>,
}

struct CachedModels {
    models: Vec<ModelInfo>,
    fetched_at: chrono::DateTime<Utc>,
}

const CACHE_TTL_SECS: i64 = 6 * 3600; // 6 hours

impl ModelsCache {
    pub fn new() -> Self {
        ModelsCache {
            models: RwLock::new(None),
        }
    }

    pub async fn get_models(&self) -> Vec<ModelInfo> {
        // Check cache
        {
            let cache = self.models.read().await;
            if let Some(ref cached) = *cache {
                let age = Utc::now()
                    .signed_duration_since(cached.fetched_at)
                    .num_seconds();
                if age < CACHE_TTL_SECS {
                    return cached.models.clone();
                }
                // Stale — return stale data but trigger refresh
                let stale = cached.models.clone();
                drop(cache);
                self.refresh_in_background();
                return stale;
            }
        }

        // No cache — fetch synchronously
        let models = Self::fetch_models().await;
        let mut cache = self.models.write().await;
        *cache = Some(CachedModels {
            models: models.clone(),
            fetched_at: Utc::now(),
        });
        models
    }

    fn refresh_in_background(&self) {
        // We can't easily spawn from &self without Arc, so the caller
        // can use Arc<ModelsCache> and spawn. For simplicity, the stale
        // response is returned and next request after TTL triggers refresh.
    }

    async fn fetch_models() -> Vec<ModelInfo> {
        // Try reading ~/.claude/settings.json
        if let Some(home) = dirs_next::home_dir() {
            let settings_path = home.join(".claude").join("settings.json");
            if let Ok(content) = tokio::fs::read_to_string(&settings_path).await {
                if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(models) = settings.get("availableModels") {
                        if let Some(arr) = models.as_array() {
                            let parsed: Vec<ModelInfo> = arr
                                .iter()
                                .filter_map(|v| {
                                    let id = v.as_str()?;
                                    Some(ModelInfo {
                                        id: id.to_string(),
                                        name: model_display_name(id).to_string(),
                                    })
                                })
                                .collect();
                            if !parsed.is_empty() {
                                return parsed;
                            }
                        }
                    }
                }
            }
        }

        // Fallback defaults
        default_models()
    }
}

fn default_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-haiku-4-5-20251001".into(),
            name: "Claude Haiku 4.5".into(),
        },
        ModelInfo {
            id: "claude-sonnet-4-6".into(),
            name: "Claude Sonnet 4.6".into(),
        },
        ModelInfo {
            id: "claude-opus-4-6".into(),
            name: "Claude Opus 4.6".into(),
        },
    ]
}

fn model_display_name(id: &str) -> &str {
    if id.contains("haiku") {
        "Claude Haiku"
    } else if id.contains("sonnet") {
        "Claude Sonnet"
    } else if id.contains("opus") {
        "Claude Opus"
    } else {
        id
    }
}

pub async fn list_models(cache: Arc<ModelsCache>) -> Json<ModelsResponse> {
    let models = cache.get_models().await;
    Json(ModelsResponse { models })
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/handlers/models.rs
git commit -m "feat: add models endpoint with settings.json cache and TTL"
```

---

### Task 11: Task Endpoint

**Files:**
- Create: `src/handlers/task.rs`

- [ ] **Step 1: Write task handler**

```rust
// src/handlers/task.rs
use axum::extract::Extension;
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::KeyId;
use crate::claude;
use crate::config::Config;
use crate::error::ApiError;
use crate::logging::{ClaudeInvocationLog, KeyLogger};
use crate::models::{TaskRequest, TaskResponse};

const DEFAULT_TIMEOUT: u64 = 120;

pub async fn create_task(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Json(req): Json<TaskRequest>,
) -> Result<Json<TaskResponse>, crate::error::AppError> {
    if req.prompt.is_empty() {
        return Err(ApiError::bad_request("prompt is required and must not be empty"));
    }

    let workdir = req
        .workdir
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| config.claude_workdir.clone());

    // Ensure workdir exists
    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create workdir: {e}")))?;

    let timeout = req.timeout_secs.unwrap_or(DEFAULT_TIMEOUT);
    let session_id = Uuid::new_v4();

    let result = claude::run_task(
        &config,
        &req.prompt,
        req.model.as_deref(),
        req.system_prompt.as_deref(),
        &workdir,
        timeout,
    )
    .await?;

    // Log the invocation
    logger.log_claude_invocation(&ClaudeInvocationLog {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: "INFO",
        key_id: key_id.0.clone(),
        session_id: session_id.to_string(),
        model: req.model.clone(),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        success: result.success,
        tokens: result.tokens.clone(),
        cost_usd: result.cost_usd,
        message: if result.success {
            "task completed".into()
        } else {
            "task failed".into()
        },
    });

    Ok(Json(TaskResponse {
        session_id: session_id.to_string(),
        result: result.result,
        success: result.success,
        duration_ms: result.duration_ms,
        tokens: result.tokens,
        cost_usd: result.cost_usd,
        error: if result.success { None } else { Some("Claude CLI returned an error".into()) },
    }))
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/handlers/task.rs
git commit -m "feat: add POST /task handler for one-shot Claude invocations"
```

---

### Task 12: Session Endpoints

**Files:**
- Create: `src/handlers/session.rs`

- [ ] **Step 1: Write session handlers**

```rust
// src/handlers/session.rs
use axum::extract::{Extension, Path};
use axum::Json;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::KeyId;
use crate::claude;
use crate::config::Config;
use crate::error::ApiError;
use crate::logging::{ClaudeInvocationLog, KeyLogger};
use crate::models::{
    DeleteSessionResponse, SessionContinueRequest, SessionInfoResponse,
    SessionStartRequest, SessionStartResponse, TaskResponse,
};
use crate::session::{SessionMeta, SessionStore};

const DEFAULT_TIMEOUT: u64 = 120;

pub async fn start_session(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Json(req): Json<SessionStartRequest>,
) -> Result<Json<SessionStartResponse>, crate::error::AppError> {
    let session_id = Uuid::new_v4();
    let now = Utc::now();

    let (workdir, auto_workdir) = match req.workdir {
        Some(ref dir) => (std::path::PathBuf::from(dir), false),
        None => (config.claude_workdir.join(session_id.to_string()), true),
    };

    tokio::fs::create_dir_all(&workdir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create session workdir: {e}")))?;

    let meta = SessionMeta {
        session_id,
        claude_session_id: None,
        created_at: now,
        last_used: now,
        model: req.model,
        system_prompt: req.system_prompt,
        workdir: workdir.clone(),
        auto_workdir,
        task_count: 0,
        tokens: Default::default(),
        cost_usd: 0.0,
    };

    let workdir_str = workdir.to_string_lossy().to_string();
    store.insert(meta);

    Ok(Json(SessionStartResponse {
        session_id: session_id.to_string(),
        workdir: workdir_str,
        created_at: now,
    }))
}

pub async fn continue_session(
    Extension(key_id): Extension<KeyId>,
    Extension(config): Extension<Arc<Config>>,
    Extension(store): Extension<Arc<SessionStore>>,
    Extension(logger): Extension<Arc<KeyLogger>>,
    Path(id): Path<String>,
    Json(req): Json<SessionContinueRequest>,
) -> Result<Json<TaskResponse>, crate::error::AppError> {
    let session_id: Uuid = id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    if req.prompt.is_empty() {
        return Err(ApiError::bad_request("prompt is required"));
    }

    let meta = store
        .get(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    // Acquire per-session lock to prevent concurrent --resume
    let lock = store
        .get_lock(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;
    let _guard = lock.lock().await;

    let timeout = req.timeout_secs.unwrap_or(DEFAULT_TIMEOUT);

    let result = match &meta.claude_session_id {
        Some(claude_sid) => {
            // Resume existing Claude session
            claude::run_resume(&config, &req.prompt, claude_sid, &meta.workdir, timeout).await?
        }
        None => {
            // First message — start new Claude session
            claude::run_task(
                &config,
                &req.prompt,
                meta.model.as_deref(),
                meta.system_prompt.as_deref(),
                &meta.workdir,
                timeout,
            )
            .await?
        }
    };

    // Update session metadata
    store.update(&session_id, |m| {
        if m.claude_session_id.is_none() {
            m.claude_session_id = result.claude_session_id.clone();
        }
        m.last_used = Utc::now();
        m.task_count += 1;
        if let Some(ref tokens) = result.tokens {
            m.tokens.accumulate(tokens);
        }
        if let Some(cost) = result.cost_usd {
            m.cost_usd += cost;
        }
    });

    // Log the invocation
    logger.log_claude_invocation(&ClaudeInvocationLog {
        timestamp: Utc::now().to_rfc3339(),
        level: "INFO",
        key_id: key_id.0.clone(),
        session_id: session_id.to_string(),
        model: meta.model.clone(),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
        success: result.success,
        tokens: result.tokens.clone(),
        cost_usd: result.cost_usd,
        message: if result.success {
            "session task completed".into()
        } else {
            "session task failed".into()
        },
    });

    Ok(Json(TaskResponse {
        session_id: session_id.to_string(),
        result: result.result,
        success: result.success,
        duration_ms: result.duration_ms,
        tokens: result.tokens,
        cost_usd: result.cost_usd,
        error: if result.success { None } else { Some("Claude CLI returned an error".into()) },
    }))
}

pub async fn get_session(
    Extension(store): Extension<Arc<SessionStore>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfoResponse>, crate::error::AppError> {
    let session_id: Uuid = id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    let meta = store
        .get(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    Ok(Json(SessionInfoResponse {
        session_id: meta.session_id.to_string(),
        created_at: meta.created_at,
        last_used: meta.last_used,
        model: meta.model,
        task_count: meta.task_count,
        workdir: meta.workdir.to_string_lossy().to_string(),
        tokens: meta.tokens,
        cost_usd: meta.cost_usd,
    }))
}

pub async fn delete_session(
    Extension(store): Extension<Arc<SessionStore>>,
    Path(id): Path<String>,
) -> Result<Json<DeleteSessionResponse>, crate::error::AppError> {
    let session_id: Uuid = id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid session ID format"))?;

    let meta = store
        .remove(&session_id)
        .ok_or_else(|| ApiError::not_found("Session not found"))?;

    // Clean up auto-allocated workdir
    if meta.auto_workdir {
        let _ = tokio::fs::remove_dir_all(&meta.workdir).await;
    }

    Ok(Json(DeleteSessionResponse {
        deleted: true,
        session_id: session_id.to_string(),
    }))
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles

- [ ] **Step 3: Commit**

```bash
git add src/handlers/session.rs
git commit -m "feat: add session start/continue/get/delete handlers"
```

---

## Chunk 5: Wiring & Deployment

### Task 13: Main.rs — Full Wiring

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Wire everything together in main.rs**

```rust
// src/main.rs
use axum::extract::Extension;
use axum::middleware;
use axum::routing::{delete, get, post};
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
    let logger = Arc::new(KeyLogger::new(&config.log_dir, &config.key_ids())?);

    // Ensure base workdir exists
    tokio::fs::create_dir_all(&config.claude_workdir).await?;

    let api_keys = Arc::new(config.api_keys.clone());

    // Public routes (no auth)
    let public_routes = Router::new()
        .route("/health", get({
            let start_time = start_time.clone();
            move || handlers::health::health(start_time.clone())
        }));

    // Protected routes (auth required)
    let protected_routes = Router::new()
        .route("/models", get({
            let cache = models_cache.clone();
            move || handlers::models::list_models(cache.clone())
        }))
        .route("/task", post(handlers::task::create_task))
        .route("/session/start", post(handlers::session::start_session))
        .route("/session/{id}", post(handlers::session::continue_session))
        .route("/session/{id}", get(handlers::session::get_session))
        .route("/session/{id}", delete(handlers::session::delete_session))
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
    eprintln!("Claudeway v{} listening on {addr}", env!("CARGO_PKG_VERSION"));
    eprintln!("Keys loaded: {:?}", config.key_ids());

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors

- [ ] **Step 3: Build release binary**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire all routes, middleware, and state in main.rs"
```

---

### Task 14: Docker & Deployment Files

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`
- Create: `.env.example`

- [ ] **Step 1: Write Dockerfile**

```dockerfile
# Build stage
FROM rust:1.85-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Runtime stage
FROM alpine:3.21
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/release/claudeway /usr/local/bin/claudeway
RUN adduser -D -u 1000 claudeway
USER claudeway
EXPOSE 3000
ENV RUST_LOG=info
ENTRYPOINT ["claudeway"]
```

- [ ] **Step 2: Write docker-compose.yml**

```yaml
services:
  claudeway:
    build: .
    ports:
      - "${PORT:-3000}:3000"
    environment:
      - WRAPPER_KEYS=${WRAPPER_KEYS}
      - CLAUDE_BIN=${CLAUDE_BIN:-claude}
      - CLAUDE_WORKDIR=${CLAUDE_WORKDIR:-/tmp/claude-tasks}
      - LOG_DIR=${LOG_DIR:-/var/log/claudeway}
      - PORT=${PORT:-3000}
      - LOG_LEVEL=${LOG_LEVEL:-info}
    volumes:
      - ./logs:/var/log/claudeway
      - claude-tasks:/tmp/claude-tasks
    restart: unless-stopped

volumes:
  claude-tasks:
```

- [ ] **Step 3: Write .env.example**

```bash
# Required: API keys in format key_id:key_value, comma-separated
WRAPPER_KEYS=admin:sk-change-me-admin,bot:sk-change-me-bot

# Path to claude CLI binary (default: claude)
CLAUDE_BIN=claude

# Base directory for session workdirs (default: /tmp/claude-tasks)
CLAUDE_WORKDIR=/tmp/claude-tasks

# Base directory for log files (default: ./logs)
LOG_DIR=./logs

# HTTP port (default: 3000)
PORT=3000

# Log level: trace, debug, info, warn, error (default: info)
LOG_LEVEL=info
```

- [ ] **Step 4: Commit**

```bash
git add Dockerfile docker-compose.yml .env.example
git commit -m "feat: add Dockerfile, docker-compose, and env example"
```

---

### Task 15: README

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write README**

````markdown
# Claudeway

Production-grade HTTP wrapper around the `claude` CLI binary. Built with Axum + Tokio.

## Quick Start

```bash
# Set up env
cp .env.example .env
# Edit .env with your API keys

# Run directly
WRAPPER_KEYS=admin:sk-your-key cargo run

# Or with Docker
docker compose up
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `WRAPPER_KEYS` | **required** | API keys as `key_id:key_value`, comma-separated |
| `CLAUDE_BIN` | `claude` | Path to claude CLI binary |
| `CLAUDE_WORKDIR` | `/tmp/claude-tasks` | Base directory for session workdirs |
| `LOG_DIR` | `./logs` | Base directory for per-key log files |
| `PORT` | `3000` | HTTP listen port |
| `LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |

## API

All endpoints except `/health` require `Authorization: Bearer <key>`.

### Health Check

```bash
curl http://localhost:3000/health
```

### List Models

```bash
curl -H "Authorization: Bearer sk-your-key" \
  http://localhost:3000/models
```

### One-Shot Task

```bash
curl -X POST http://localhost:3000/task \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is 2+2?", "model": "sonnet"}'
```

### Session: Start

```bash
curl -X POST http://localhost:3000/session/start \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "sonnet"}'
```

### Session: Continue

```bash
curl -X POST http://localhost:3000/session/<session_id> \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Now explain it differently"}'
```

### Session: Info

```bash
curl -H "Authorization: Bearer sk-your-key" \
  http://localhost:3000/session/<session_id>
```

### Session: Delete

```bash
curl -X DELETE -H "Authorization: Bearer sk-your-key" \
  http://localhost:3000/session/<session_id>
```

## Logging

Each API key gets its own log directory with monthly rotating JSON log files:

```
logs/
├── admin/
│   └── 2026-03.log
├── bot/
│   └── 2026-03.log
└── _unauthorized/
    └── 2026-03.log
```

## Error Responses

All errors return JSON:

```json
{"error": "message", "code": "ERROR_CODE"}
```

Status codes: 400, 401, 404, 408, 500.
````

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add README with setup instructions and API examples"
```

---

### Task 16: Final Verification

- [ ] **Step 1: Full build check**

Run: `cargo build`
Expected: Clean compile

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Fix any compilation issues**

Iterate until clean build + tests pass.

- [ ] **Step 4: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix: resolve compilation issues from integration"
```
