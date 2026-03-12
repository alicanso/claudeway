# Claudeway: Rust HTTP Wrapper for Claude CLI

## Overview
Production-grade Axum HTTP server wrapping the `claude` CLI binary. Provides REST API for one-shot tasks and persistent sessions with per-key auth, structured JSON logging, and token/cost tracking.

## Stack
- **Axum** (HTTP framework)
- **Tokio** (async runtime)
- **Serde / serde_json**
- **DashMap** (concurrent session store)
- **Tower-HTTP** (middleware: auth, tracing, timeout)
- **UUID v4** for session IDs
- **Anyhow** for error handling
- **Chrono** for timestamps
- **tracing + tracing-subscriber** for logging
- **tracing-appender** for file-based log rotation

## API Key Management

Multiple API keys supported. Define via env var:

```
WRAPPER_KEYS=admin:sk-abc123,bot:sk-def456
```

Format: `{key_id}:{key_value}` comma-separated. Parse on startup into a `HashMap<String, String>` (value -> id).

Auth middleware: check `Authorization: Bearer {value}`. If match, extract `key_id` and attach to request extensions. Return 401 if no match.

## Logging

### Format
JSON structured. Every log line is a valid JSON object:
```json
{
  "timestamp": "2026-03-12T10:00:00Z",
  "level": "INFO",
  "key_id": "admin",
  "method": "POST",
  "path": "/task",
  "duration_ms": 843,
  "status": 200,
  "session_id": "uuid",
  "claude_exit_code": 0,
  "tokens": {
    "input": 1240,
    "output": 380,
    "cache_read": 820,
    "cache_write": 0
  },
  "cost_usd": 0.0043,
  "message": "task completed"
}
```

### Token extraction
After each Claude CLI invocation:
1. Parse `total_cost_usd` and `duration_ms` from `--output-format json` stdout
2. Read the corresponding session JSONL file at `~/.claude/projects/{project_hash}/{session_id}.jsonl`
3. Extract the last entry's token breakdown: `input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`
4. If JSONL read fails, log available fields only (never block on this)

### Destination
File only. Base log dir from env var `LOG_DIR` (default: `./logs`).

### Structure
Each key gets its own directory and monthly rotating log file:
```
logs/
├── admin/
│   └── 2026-03.log
├── bot/
│   └── 2026-03.log
└── _unauthorized/
    └── 2026-03.log
```

File naming: `{YYYY-MM}.log`. Rotate on month boundary.

### What to log
Every request:
- `key_id`, `method`, `path`, `status`, `duration_ms`

Every Claude CLI invocation:
- `key_id`, `session_id`, `model`, `exit_code`, `duration_ms`, `success`, `tokens`, `cost_usd`

Auth failures -> `logs/_unauthorized/YYYY-MM.log`:
- `timestamp`, `method`, `path`, `remote_addr`

### Implementation
Use `tracing-appender` with a custom per-key writer. After resolving `key_id`, route log output to the appropriate file. Use `tracing` Span to carry `key_id` through the entire request lifecycle including the Claude subprocess call.

## Endpoints

### `GET /health`
Returns 200 OK with uptime and version. No auth required.

### `GET /models`
Returns available Claude models dynamically. Resolution order:
1. Read `~/.claude/settings.json` -> `availableModels` field if present
2. Fallback: hardcoded default list (`haiku`, `sonnet`, `opus` with full model IDs)

Cache with 6-hour TTL. Return stale cache while refreshing in background.

### `POST /task`
One-shot Claude task, no session persistence.

Request:
```json
{
  "prompt": "string (required)",
  "model": "sonnet | haiku | opus | full-model-id (optional, default: sonnet)",
  "system_prompt": "string (optional)",
  "workdir": "string (optional, default: CLAUDE_WORKDIR)",
  "timeout_secs": 120
}
```

Response:
```json
{
  "session_id": "uuid",
  "result": "string",
  "success": true,
  "duration_ms": 1234,
  "tokens": {
    "input": 1240,
    "output": 380,
    "cache_read": 820,
    "cache_write": 0
  },
  "cost_usd": 0.0043,
  "error": null
}
```

Runs:
```bash
claude -p "{prompt}" --output-format json [--model model] [--system-prompt "..."]
```

### `POST /session/start`
Create persistent session. Allocates isolated workdir under `CLAUDE_WORKDIR/{uuid}/`.

Request:
```json
{
  "model": "sonnet (optional)",
  "system_prompt": "string (optional)",
  "workdir": "string (optional)"
}
```

Response:
```json
{
  "session_id": "uuid",
  "workdir": "/path/to/dir",
  "created_at": "ISO8601"
}
```

### `POST /session/:id`
Continue existing session via `--resume {claude_session_id}`.

Wrapper UUID -> Claude CLI session ID stored in `SessionMeta`. Per-session `Mutex<()>` prevents concurrent `--resume` race conditions.

Request:
```json
{
  "prompt": "string (required)",
  "timeout_secs": 120
}
```

Response: same shape as `/task`.

### `GET /session/:id`
Returns session metadata: `created_at`, `last_used`, `model`, `task_count`, `workdir`, cumulative `tokens`, cumulative `cost_usd`.

### `DELETE /session/:id`
Remove session from store, delete auto-allocated workdir.

## Configuration (env vars)
| Var | Default | Description |
|---|---|---|
| `WRAPPER_KEYS` | required | Comma-separated `key_id:key_value` pairs |
| `CLAUDE_BIN` | `claude` | Path to claude binary |
| `CLAUDE_WORKDIR` | `/tmp/claude-tasks` | Base dir for session workdirs |
| `LOG_DIR` | `./logs` | Base dir for log files |
| `PORT` | `3000` | HTTP port |
| `LOG_LEVEL` | `info` | Tracing log level |

## Error handling
All errors return JSON:
```json
{ "error": "message", "code": "ERROR_CODE" }
```

Status codes: 400 bad request, 401 unauthorized, 404 not found, 408 timeout, 500 internal.

## Concurrency
- `DashMap` for session store
- Each request spawns `tokio::process::Command`
- Per-session `Mutex<()>` for `--resume` serialization

## Project structure
```
├── Cargo.toml
└── src/
    ├── main.rs
    ├── config.rs
    ├── models.rs
    ├── logging.rs
    ├── auth.rs
    ├── handlers/
    │   ├── mod.rs
    │   ├── health.rs
    │   ├── models.rs
    │   ├── task.rs
    │   └── session.rs
    ├── claude.rs
    ├── session.rs
    └── error.rs
```

Also:
- `Dockerfile` (multi-stage, alpine final image)
- `docker-compose.yml` with all env var placeholders
- `.env.example` with all vars documented
- `README.md` with setup instructions, env vars, and curl examples
