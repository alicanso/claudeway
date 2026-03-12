# Telegram Repo Selection Design

## Overview

When a user sends a message in the Telegram group's General section (no topic), the bot lists available GitHub repositories and lets the user pick one. The selected repo becomes the working directory for that session's Forum Topic.

## Flow

1. User sends a message in General (no topic thread).
2. Bot stores the original message text as the pending prompt.
3. Bot runs `gh repo list` for the user's account and all accessible organizations.
4. Bot sends a numbered list of repositories (paginated, 20 per page).
5. User replies with a number to select a repo.
6. Bot resolves the local path: `{projects_dir}/{owner}/{repo}`.
   - If the directory exists: run `git pull` to update.
   - If not: run `git clone {clone_url} {path}`.
7. Bot creates a Forum Topic named after the repo (e.g., `alicanso/claudeway`).
8. Bot sets the session's `workdir` to the repo path.
9. Bot sends the original prompt to Claude with that workdir.

## Repo Discovery

```bash
for owner in "$(gh api user -q .login)" $(gh org list | awk '{print $1}'); do
  gh repo list "$owner" --limit 1000
done
```

This lists repos from:
- The authenticated user's personal account
- All organizations the user has access to

Output is parsed to extract `owner/repo` and clone URL.

## Pagination

- 20 repos per page.
- If more than 20, append instructions: "Send `/next` for more, or type a number to select."
- `/next` advances to the next page, `/prev` goes back.

## Config

Add `projects_dir` to the Telegram plugin config in `claudeway.toml`:

```toml
[plugins.telegram]
enabled = true
bot_token = "..."
chat_id = "..."
projects_dir = "~/Documents/GitHub"
```

Default value: `~/Documents/GitHub`.

## Directory Structure

Repos are stored in `{projects_dir}/{owner}/{repo}`:

```
~/Documents/GitHub/
  alicanso/claudeway/
  alicanso/other-repo/
  some-org/org-repo/
```

## Session State Changes

`SessionInfo` gains a new field:

```rust
pub struct SessionInfo {
    pub claude_session_id: Option<String>,
    pub workdir: PathBuf,
    pub lock: Arc<tokio::sync::Mutex<()>>,
    pub pending_prompt: Option<String>,  // stored until repo is selected
}
```

A new state enum tracks the topic lifecycle:

```rust
enum TopicState {
    AwaitingRepoSelection { pending_prompt: String, page: usize },
    Active,
}
```

When `TopicState` is `AwaitingRepoSelection`, numeric messages select a repo instead of going to Claude.

## Repo List State

```rust
struct RepoListState {
    repos: Vec<RepoInfo>,
    page: usize,
}

struct RepoInfo {
    full_name: String,   // "owner/repo"
    clone_url: String,   // "https://github.com/owner/repo.git"
}
```

Stored temporarily per-topic until selection is made, then discarded.

## Commands

- `/repos` — re-display the repo list in the current topic (allows switching repos mid-session)
- `/next` — next page of repo list
- `/prev` — previous page of repo list
- `/close` — existing behavior (close topic, remove session)

## Error Handling

- `gh` CLI not installed or not authenticated: send error message to user.
- `git clone` fails: send error message, keep topic in `AwaitingRepoSelection`.
- `git pull` fails: warn but proceed (repo may have local changes).
- No repos found: send "No repositories found" message.

## Security

- Only messages from the configured `chat_id` are processed (existing check).
- `projects_dir` is server-side config, not user-controllable via Telegram.
- Clone URLs come from `gh` CLI (authenticated), not user input.
