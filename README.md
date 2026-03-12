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
