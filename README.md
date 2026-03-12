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

**~3 MB binary** &nbsp;&bull;&nbsp; **Alpine Docker image** &nbsp;&bull;&nbsp; **Lock-free concurrent sessions**

</div>

<br />

---

## Table of Contents

- [Why Claudeway?](#why-claudeway)
- [Quick Start](#quick-start)
- [Examples](#examples)
  - [One-shot Code Review](#one-shot-code-review)
  - [Multi-turn Session](#multi-turn-session)
  - [CI/CD Pipeline Integration](#cicd-pipeline-integration)
  - [Batch Processing](#batch-processing)
  - [Cost Tracking Dashboard](#cost-tracking-dashboard)
- [API Reference](#api-reference)
  - [GET /health](#get-health)
  - [GET /models](#get-models)
  - [POST /task](#post-task)
  - [Sessions](#sessions)
- [Configuration](#configuration)
  - [API Keys](#api-keys)
- [Logging](#logging)
- [Performance](#performance)
- [Architecture](#architecture)
- [Deployment](#deployment)
- [Error Responses](#error-responses)
- [License](#license)

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
| **Deploy anywhere** | Single static binary. Alpine Docker image. Zero config to start. |

## Quick Start

### 1. Install the Claude CLI

Claudeway is an HTTP gateway for the [Claude CLI](https://docs.anthropic.com/en/docs/claude-cli). Install it first:

```bash
npm install -g @anthropic-ai/claude-code
```

### 2. Run Claudeway

No configuration required. Claudeway generates an API key for you on startup.

**Docker** (no Rust required):

```bash
docker run -p 3000:3000 claudeway
```

**Pre-built binary** (no Rust required):

```bash
# macOS (Apple Silicon)
curl -fsSL https://github.com/alicanso/claudeway/releases/latest/download/claudeway-aarch64-apple-darwin -o claudeway && chmod +x claudeway && ./claudeway

# macOS (Intel)
curl -fsSL https://github.com/alicanso/claudeway/releases/latest/download/claudeway-x86_64-apple-darwin -o claudeway && chmod +x claudeway && ./claudeway

# Linux (x86_64)
curl -fsSL https://github.com/alicanso/claudeway/releases/latest/download/claudeway-x86_64-unknown-linux-musl -o claudeway && chmod +x claudeway && ./claudeway
```

**From source** (Rust developers):

```bash
cargo install --git https://github.com/alicanso/claudeway
claudeway
```

On startup you'll see:

```
  No API keys configured — generated one for you:

    sk-a7f3b2e19c...

  Use it as: curl -H "Authorization: Bearer sk-a7f3b2e19c..." http://localhost:3000/task
  To set your own keys, use --keys or WRAPPER_KEYS env var.

Claudeway v0.1.0 listening on 0.0.0.0:3000
```

### 3. Verify

```bash
# Health check (no auth required)
curl http://localhost:3000/health

# List models (use the key printed at startup)
curl -H "Authorization: Bearer sk-a7f3b2e19c..." http://localhost:3000/models
```

## Examples

### One-shot Code Review

Send a file for instant code review — no session needed.

```bash
curl -X POST http://localhost:3000/task \
  -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "Review this code for bugs, security issues, and performance:\n\n'"$(cat src/main.rs)"'",
    "model": "sonnet"
  }'
```

### Multi-turn Session

Build a stateful conversation — Claude remembers the full context across messages.

```bash
# Start a session
SESSION=$(curl -s -X POST http://localhost:3000/session/start \
  -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model": "sonnet"}' | jq -r '.session_id')

# First message — set the context
curl -s -X POST http://localhost:3000/session/$SESSION \
  -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "I have a Rust web app using Axum. I need to add rate limiting."}'

# Follow-up — Claude remembers the previous context
curl -s -X POST http://localhost:3000/session/$SESSION \
  -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Now add per-IP tracking with a sliding window algorithm."}'

# Check cumulative cost
curl -s -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  http://localhost:3000/session/$SESSION | jq '{cost_usd, tokens}'

# Clean up
curl -s -X DELETE -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  http://localhost:3000/session/$SESSION
```

### CI/CD Pipeline Integration

Automate code review in your GitHub Actions workflow.

```yaml
# .github/workflows/ai-review.yml
name: AI Code Review
on: [pull_request]

jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Get diff
        run: git diff origin/main...HEAD > /tmp/diff.txt

      - name: AI Review
        run: |
          RESPONSE=$(curl -s -X POST ${{ secrets.CLAUDEWAY_URL }}/task \
            -H "Authorization: Bearer ${{ secrets.CLAUDEWAY_KEY }}" \
            -H "Content-Type: application/json" \
            -d "$(jq -n --arg diff "$(cat /tmp/diff.txt)" '{
              prompt: ("Review this PR diff. Flag bugs, security issues, and suggest improvements:\n\n" + $diff),
              model: "sonnet",
              timeout_secs: 300
            }')")
          echo "$RESPONSE" | jq -r '.result'
```

### Batch Processing

Process multiple files in parallel using `xargs`.

```bash
# Analyze all Python files in a project
find ./src -name "*.py" | xargs -P 4 -I {} sh -c '
  RESULT=$(curl -s -X POST http://localhost:3000/task \
    -H "Authorization: Bearer $CLAUDEWAY_KEY" \
    -H "Content-Type: application/json" \
    -d "{
      \"prompt\": \"Analyze this file for type safety issues and suggest type hints:\\n\\n$(cat {})\",
      \"model\": \"haiku\"
    }")
  echo "=== {} ==="
  echo "$RESULT" | jq -r ".result"
'
```

### Cost Tracking Dashboard

Monitor usage and cost across all sessions.

```bash
# Get cost for a specific session
curl -s -H "Authorization: Bearer $CLAUDEWAY_KEY" \
  http://localhost:3000/session/$SESSION_ID | jq '{
    task_count,
    total_tokens: (.tokens.input + .tokens.output),
    cost_usd
  }'

# Parse daily costs from logs
cat logs/admin/2026-03.log | jq -s '
  group_by(.timestamp[:10]) |
  map({
    date: .[0].timestamp[:10],
    requests: length,
    total_cost: (map(.cost_usd) | add),
    total_tokens: (map(.tokens.input + .tokens.output) | add)
  })
'
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

Every option can be set via CLI flags, environment variables, or both. CLI flags take precedence.

| Flag | Env Variable | Default | Description |
|---|---|---|---|
| `--keys` | `WRAPPER_KEYS` | *auto-generated* | API keys as `key_id:secret`, comma-separated |
| `--claude-bin` | `CLAUDE_BIN` | `claude` | Path to claude CLI binary |
| `--workdir` | `CLAUDE_WORKDIR` | `/tmp/claude-tasks` | Base directory for session workdirs |
| `--log-dir` | `LOG_DIR` | `./logs` | Base directory for per-key log files |
| `-p, --port` | `PORT` | `3000` | HTTP listen port |
| `--log-level` | `LOG_LEVEL` | `info` | `trace` / `debug` / `info` / `warn` / `error` |

### API Keys

If you don't provide `--keys` or `WRAPPER_KEYS`, Claudeway generates a single key on startup and prints it to stderr.

For production, define your own keys. Each key has a **key ID** (a label that appears in logs) and a **secret** (the Bearer token used in requests):

```bash
# Generate a secure secret
openssl rand -hex 32

# Use it
claudeway --keys "admin:$(openssl rand -hex 32)"

# Multiple keys
claudeway --keys "admin:sk-prod-key-001,ci-bot:sk-ci-key-002"

# Or via environment variable
export WRAPPER_KEYS=admin:sk-prod-key-001,ci-bot:sk-ci-key-002
claudeway
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

## Performance

Claudeway adds virtually zero latency on top of the Claude CLI:

- **Axum** — the fastest Rust HTTP framework, built on hyper and Tokio
- **DashMap** — lock-free concurrent hashmap for session storage
- **Zero-copy routing** — compile-time route resolution, no regex matching
- **Per-session Mutex** — prevents `--resume` race conditions without global locks
- **Async I/O everywhere** — non-blocking process spawning, file I/O, and networking

The bottleneck is always Claude, never Claudeway.

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

## Deployment

For production deployments:

```bash
# Optimized release build (~3 MB)
cargo build --release

# With Swagger UI at /docs (~14 MB)
cargo build --release --features swagger

# Docker Compose
cp .env.example .env    # edit with your keys
docker compose up -d
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

## License

MIT
