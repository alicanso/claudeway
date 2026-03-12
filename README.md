<div align="center">

# Claudeway

**Production-grade HTTP wrapper around the Claude CLI**

[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-ready-2496ED?logo=docker&logoColor=white)](Dockerfile)

Turn the `claude` CLI into a REST API with multi-key auth, persistent sessions, per-key structured logging, and token/cost tracking.

</div>

---

## Features

- **One-shot tasks** and **persistent sessions** via REST API
- **Multi-key auth** with per-key isolated logging
- **Token & cost tracking** per request and cumulative per session
- **Model caching** with background refresh
- **Concurrent-safe** session management with per-session locking
- **Monthly rotating** JSON log files per API key
- **Docker-ready** with multi-stage Alpine build

## Quick Start

```bash
# Clone and run
git clone https://github.com/alicansoysal/claudeway.git
cd claudeway
WRAPPER_KEYS=admin:sk-your-key cargo run
```

```bash
# Or with Docker
cp .env.example .env  # edit with your keys
docker compose up
```

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

One-shot Claude task. No session state.

```bash
curl -X POST http://localhost:3000/task \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Explain monads in one sentence", "model": "sonnet"}'
```
```json
{
  "session_id": "uuid",
  "result": "A monad is a design pattern...",
  "success": true,
  "duration_ms": 1832,
  "tokens": { "input": 24, "output": 156, "cache_read": 0, "cache_write": 0 },
  "cost_usd": 0.0021,
  "error": null
}
```

### Sessions

Persistent, stateful conversations with automatic workdir isolation.

```bash
# Start a session
curl -X POST http://localhost:3000/session/start \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"model": "sonnet"}'

# Continue the conversation
curl -X POST http://localhost:3000/session/<session_id> \
  -H "Authorization: Bearer sk-your-key" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Now explain it differently"}'

# Check session stats
curl -H "Authorization: Bearer sk-your-key" \
  http://localhost:3000/session/<session_id>

# Clean up
curl -X DELETE -H "Authorization: Bearer sk-your-key" \
  http://localhost:3000/session/<session_id>
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `WRAPPER_KEYS` | *required* | API keys as `key_id:key_value`, comma-separated |
| `CLAUDE_BIN` | `claude` | Path to claude CLI binary |
| `CLAUDE_WORKDIR` | `/tmp/claude-tasks` | Base directory for session workdirs |
| `LOG_DIR` | `./logs` | Base directory for per-key log files |
| `PORT` | `3000` | HTTP listen port |
| `LOG_LEVEL` | `info` | `trace` / `debug` / `info` / `warn` / `error` |

## Logging

Each API key gets its own directory with monthly rotating JSON log files:

```
logs/
├── admin/
│   └── 2026-03.log          # one JSON object per line
├── bot/
│   └── 2026-03.log
└── _unauthorized/
    └── 2026-03.log
```

Every log entry includes timestamps, key ID, request details, and for Claude invocations: exit code, token breakdown, and cost.

## Architecture

```
                    ┌──────────────┐
                    │   Axum HTTP  │
                    │    Server    │
                    └──────┬───────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
         ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
         │  Auth   │  │ Health  │  │ Models  │
         │Midlware │  │  (pub)  │  │ Cache   │
         └────┬────┘  └─────────┘  └─────────┘
              │
    ┌─────────┼──────────┐
    │         │          │
┌───▼───┐ ┌──▼───┐ ┌────▼────┐
│ /task │ │/sess.│ │Per-Key  │
│Handler│ │Handle│ │ Logger  │
└───┬───┘ └──┬───┘ └─────────┘
    │        │
    └───┬────┘
        │
   ┌────▼─────┐     ┌──────────┐
   │ Claude   │────▶│claude CLI│
   │ Executor │     │ process  │
   └──────────┘     └──────────┘
```

## Error Responses

All errors return a consistent JSON shape:

```json
{ "error": "description", "code": "ERROR_CODE" }
```

| Status | Code | When |
|--------|------|------|
| 400 | `BAD_REQUEST` | Invalid request body or parameters |
| 401 | `UNAUTHORIZED` | Missing or invalid API key |
| 404 | `NOT_FOUND` | Session not found |
| 408 | `TIMEOUT` | Claude CLI exceeded timeout |
| 500 | `INTERNAL_ERROR` | Unexpected server error |

## License

MIT
