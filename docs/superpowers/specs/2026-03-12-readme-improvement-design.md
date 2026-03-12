# README Improvement Design

## Problem

The current README has three issues:

1. **Quick Start assumes Rust knowledge** — `cargo run` is the primary path, alienating users who just want to run Claudeway
2. **Wrapper Keys are confusing** — `WRAPPER_KEYS=admin:sk-your-key` format is not explained; users don't know what key_id vs key_value means or how to generate secure values
3. **Deployment section duplicates Quick Start** — Docker and binary instructions appear in both sections

## Design

### Quick Start Restructure

Replace the current Quick Start with a step-by-step tutorial:

#### Step 1: Prerequisites

Explain that Claudeway wraps the Claude CLI, so it must be installed first. Link to official Claude CLI docs. Provide install command: `npm install -g @anthropic-ai/claude-code`.

#### Step 2: Generate API Keys

Explain the `key_id:secret` format clearly:

- **key_id**: A label for audit logs (e.g., `admin`, `ci-bot`). Not a secret.
- **secret**: The Bearer token used in HTTP requests. Must be kept secret.

Provide `openssl rand -hex 32` as the recommended way to generate a secure secret. Show a concrete example of setting `WRAPPER_KEYS` with one key and with multiple keys.

#### Step 3: Run

Three options, equally weighted:

1. **Docker** (no Rust required) — `docker run` one-liner
2. **Pre-built binary** (no Rust required) — download from GitHub Releases
3. **From source** (Rust developers) — `cargo install --git` or `cargo run`

#### Step 4: Verify

Two curl commands: health check (no auth) and models endpoint (with auth). Shows the server is running and auth is working.

### Configuration Table Update

Change `WRAPPER_KEYS` description from `API keys as key_id:key_value, comma-separated` to a clearer description that references the Quick Start section for setup instructions.

### Deployment Section

Remove duplicated Docker/binary instructions. Keep only production-specific details:

- `cargo build --release` optimization note
- `docker compose` for production deployments
- Reference Quick Start for basic setup

### No Changes

These sections remain unchanged:

- Why Claudeway?
- Performance
- API Reference
- Sessions
- Logging
- Architecture
- Error Responses
- License
