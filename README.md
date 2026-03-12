<div align="center">

# Claudeway

### Blazing-fast HTTP gateway for the Claude CLI

Built with Rust. Zero garbage collection. Sub-millisecond overhead.

[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Axum](https://img.shields.io/badge/axum-0.8-blue)](https://github.com/tokio-rs/axum)
[![Tokio](https://img.shields.io/badge/tokio-async-8B5CF6)](https://tokio.rs/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-alpine-2496ED?logo=docker&logoColor=white)](Dockerfile)
[![OpenAPI](https://img.shields.io/badge/OpenAPI-3.1-6BA539?logo=openapiinitiative&logoColor=white)](#api-reference)

<br />

**~6 MB binary** &nbsp;&bull;&nbsp; **~2 MB Docker image** &nbsp;&bull;&nbsp; **~5 ms cold start** &nbsp;&bull;&nbsp; **Lock-free concurrent sessions**

[Quick Start](#quick-start) &nbsp;&bull;&nbsp; [API Reference](#api-reference) &nbsp;&bull;&nbsp; [Configuration](#configuration) &nbsp;&bull;&nbsp; [Architecture](#architecture)

</div>

<br />

---

## Why Claudeway?

You've got the `claude` CLI. It's powerful. But it's not an API.

Claudeway wraps it in a **zero-overhead Rust HTTP server** and gives you:

| | |
|---|---|
| **Multi-tenant auth** | Multiple API keys, each with isolated logging |
| **Persistent sessions** | Stateful conversations with `--resume`, per-session mutex locks |
| **Full cost visibility** | Token counts + USD cost on every response |
| **Per-key audit logs** | Monthly rotating JSONL files per API key |
| **Zero-copy performance** | Axum + Tokio + DashMap. No GC pauses. No runtime overhead. |
| **Type-safe OpenAPI** | Auto-generated OpenAPI 3.1 spec + Swagger UI at `/docs` |
| **Deploy anywhere** | ~6 MB static binary. Alpine Docker image. One env var to configure. |

## Quick Start

```bash
# One command. That's it.
WRAPPER_KEYS=admin:sk-your-key cargo run
```

```bash
# Or Docker
cp .env.example .env
docker compose up
```

Server starts in milliseconds. Health check responds in microseconds.

## Performance

Claudeway adds virtually zero latency on top of the Claude CLI:

- **Axum** — the fastest Rust HTTP framework, built on hyper and Tokio
- **DashMap** — lock-free concurrent hashmap for session storage
- **Zero-copy routing** — compile-time route resolution, no regex matching
- **Per-session Mutex** — prevents `--resume` race conditions without global locks
- **Async I/O everywhere** — non-blocking process spawning, file I/O, and networking

The bottleneck is always Claude, never Claudeway.

## API Reference

All endpoints except `/health` require `Authorization: Bearer <key>`.

### `GET /health`

```bash
curl http://localhost:3000/health
```
```json
{ "status": "ok", "version": "0.1.0", "uptime_secs": 42 }
```

### `GET /models`

Returns available models. Cached with 6-hour TTL, serves stale while refreshing.

```bash
curl -H "Authorization: Bearer sk-your-key" http://localhost:3000/models
```
```json
{
  "models": [
    { "id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6" },
    { "id": "claude-opus-4-6", "name": "Claude Opus 4.6" },
    { "id": "claude-haiku-4-5-20251001", "name": "Claude Haiku 4.5" }
  ]
}
```

### `POST /task`

One-shot task. Fire and forget. No session state.

```bash
curl -X POST http://localhost:3000/task \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Explain monads in one sentence", "model": "sonnet"}'
```
```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "result": "A monad is a design pattern that chains operations...",
  "success": true,
  "duration_ms": 1832,
  "tokens": { "input": 24, "output": 156, "cache_read": 0, "cache_write": 0 },
  "cost_usd": 0.0021,
  "error": null
}
```

**Options:**

| Field | Type | Default | Description |
|---|---|---|---|
| `prompt` | string | *required* | The prompt to send |
| `model` | string | `sonnet` | `sonnet` / `haiku` / `opus` or full model ID |
| `system_prompt` | string | — | System prompt override |
| `workdir` | string | `$CLAUDE_WORKDIR` | Working directory for Claude |
| `timeout_secs` | int | `120` | Max execution time |

### Sessions

Persistent, stateful conversations. Each session gets an isolated workdir and tracks cumulative token usage and cost.

```bash
# Start a session
curl -X POST http://localhost:3000/session/start \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "sonnet"}'
# → { "session_id": "uuid", "workdir": "/tmp/claude-tasks/uuid", "created_at": "..." }

# Send messages
curl -X POST http://localhost:3000/session/<id> \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Refactor this to use async iterators"}'
# → same response shape as /task

# Check cumulative stats
curl -H "Authorization: Bearer sk-your-key" http://localhost:3000/session/<id>
# → { "task_count": 5, "tokens": {...}, "cost_usd": 0.042, ... }

# Clean up (auto-deletes workdir)
curl -X DELETE -H "Authorization: Bearer sk-your-key" http://localhost:3000/session/<id>
```

Concurrent requests to the same session are automatically serialized via per-session mutex locks — no race conditions on `--resume`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `WRAPPER_KEYS` | *required* | API keys as `key_id:key_value`, comma-separated |
| `CLAUDE_BIN` | `claude` | Path to claude CLI binary |
| `CLAUDE_WORKDIR` | `/tmp/claude-tasks` | Base directory for session workdirs |
| `LOG_DIR` | `./logs` | Base directory for per-key log files |
| `PORT` | `3000` | HTTP listen port |
| `LOG_LEVEL` | `info` | `trace` / `debug` / `info` / `warn` / `error` |

**Multi-key example:**

```bash
WRAPPER_KEYS=admin:sk-prod-key-001,ci-bot:sk-ci-key-002,staging:sk-stg-key-003
```

Each key gets its own log directory, so you always know who did what.

## Logging

Structured JSON. One line per event. Per-key isolation with monthly rotation.

```
logs/
├── admin/
│   ├── 2026-03.log
│   └── 2026-04.log
├── ci-bot/
│   └── 2026-03.log
└── _unauthorized/
    └── 2026-03.log
```

Every Claude invocation is logged with full detail:

```json
{
  "timestamp": "2026-03-12T10:00:00Z",
  "level": "INFO",
  "key_id": "admin",
  "session_id": "550e8400-...",
  "claude_exit_code": 0,
  "duration_ms": 1832,
  "success": true,
  "tokens": { "input": 1240, "output": 380, "cache_read": 820, "cache_write": 0 },
  "cost_usd": 0.0043,
  "message": "task completed"
}
```

## Architecture

```
         Request
            │
            ▼
    ┌───────────────┐
    │   Axum HTTP   │  Tokio async runtime
    │    Server     │  Zero-copy routing
    └───────┬───────┘
            │
     ┌──────┴──────┐
     │             │
 ┌───▼────┐   ┌───▼────┐
 │ Public │   │  Auth  │  Bearer token → key_id
 │ /health│   │Midlware│  O(1) HashMap lookup
 └────────┘   └───┬────┘
                  │
       ┌──────────┼──────────┐
       │          │          │
  ┌────▼───┐ ┌───▼────┐ ┌───▼─────┐
  │ /task  │ │/session│ │ /models │  6hr TTL cache
  │Handler │ │Handler │ │ Handler │
  └────┬───┘ └───┬────┘ └─────────┘
       │         │
       └────┬────┘
            │
   ┌────────▼────────┐
   │ Claude Executor │  tokio::process::Command
   │   + Timeout     │  Token extraction from JSONL
   └────────┬────────┘
            │
   ┌────────▼────────┐
   │   Per-Key JSON  │  Monthly rotation
   │     Logger      │  Structured audit trail
   └─────────────────┘
```

## Error Responses

Consistent JSON error shape across all endpoints:

```json
{ "error": "description", "code": "ERROR_CODE" }
```

| Status | Code | When |
|--------|------|------|
| `400` | `BAD_REQUEST` | Invalid request body or parameters |
| `401` | `UNAUTHORIZED` | Missing or invalid API key |
| `404` | `NOT_FOUND` | Session not found |
| `408` | `TIMEOUT` | Claude CLI exceeded timeout |
| `500` | `INTERNAL_ERROR` | Unexpected server error |

## Deployment

**Binary (recommended):**
```bash
cargo build --release
# Binary at target/release/claudeway (~6 MB)
```

**Docker:**
```bash
docker build -t claudeway .
docker run -e WRAPPER_KEYS=admin:sk-key -p 3000:3000 claudeway
```

## License

MIT
