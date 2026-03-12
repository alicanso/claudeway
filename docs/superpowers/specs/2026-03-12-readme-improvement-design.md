# README Improvement Design

## Problem

The current README has three issues:

1. **Quick Start assumes Rust knowledge** — `cargo run` is the primary path, alienating users who just want to run Claudeway
2. **Wrapper Keys are confusing** — `WRAPPER_KEYS=admin:sk-your-key` format is not explained; users don't understand what key_id vs key_value means or how to generate secure values
3. **Deployment section duplicates Quick Start** — Docker and binary instructions appear in both sections

## Design

### Code Changes

#### Auto-generated API Key (config.rs)
When `WRAPPER_KEYS` / `--keys` is not provided, Claudeway generates a random `sk-<64 hex chars>` key with key_id `default` and prints it to stderr on startup. This makes the quickstart zero-config.

#### CLI Arguments (clap)
All configuration options are available as both CLI flags and environment variables. CLI flags take precedence. Added via `clap` with `derive` and `env` features:

- `--keys` (env: `WRAPPER_KEYS`) — optional, auto-generates if absent
- `--claude-bin` (env: `CLAUDE_BIN`)
- `--workdir` (env: `CLAUDE_WORKDIR`)
- `--log-dir` (env: `LOG_DIR`)
- `-p, --port` (env: `PORT`)
- `--log-level` (env: `LOG_LEVEL`)

### README Changes

#### Quick Start
Step-by-step tutorial:
1. Install Claude CLI (`npm install -g @anthropic-ai/claude-code`)
2. Run Claudeway — three equally weighted options: Docker, pre-built binary, from source. No key setup needed.
3. Verify — health check + models endpoint using the auto-generated key

#### Configuration
Merged table showing both CLI flags and env vars. Separate "API Keys" subsection explaining key_id:secret format with `openssl rand -hex 32` for production use.

#### Deployment
Trimmed to production-specific details only (release build, docker compose). No duplication with Quick Start.

### No Changes
- Why Claudeway?
- Performance
- API Reference / Sessions
- Logging
- Architecture
- Error Responses
- License
