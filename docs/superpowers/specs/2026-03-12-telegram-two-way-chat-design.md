# Telegram Two-Way Chat Design

Turn the Telegram plugin from a one-way notification sender into a full two-way Claude chat interface, where each Telegram Forum Topic maps to an independent Claude session.

## Context

The current Telegram plugin only pushes notifications (session completed, request completed) to a configured chat. The goal is to let users send messages to the bot in Telegram and receive Claude responses, the same way they would interact with `claude` on the CLI.

## Architecture

### Polling Loop

On `ServerStarted`, the plugin's `on_event` handler spawns a background polling task via `tokio::spawn`. The spawned task captures `Arc` clones of the bot token, chat ID, session map, and `Arc<Config>` (obtained from `PluginContext`). The task long-polls Telegram's `getUpdates` API with `timeout=30`. This keeps a persistent connection open; Telegram returns immediately when a message arrives, so effective latency is near zero. No webhook or tunnel dependency.

The plugin stores the `JoinHandle` from the spawn so `on_shutdown` can abort it. Since `getUpdates` blocks for up to 30 seconds, `on_shutdown` calls `handle.abort()` for immediate cancellation.

**Offset tracking:** The polling loop maintains a local `offset: i64` variable, starting at 0. After processing each batch of updates, it sets `offset = last_update_id + 1` and passes it to the next `getUpdates` call. This tells Telegram to only return new updates and prevents reprocessing.

On polling error, the loop waits 5 seconds and retries.

**Subscriptions:** `on_register` must subscribe to `EventType::ServerStarted` in addition to the existing `RequestCompleted` and `SessionCompleted` subscriptions.

### Session Mapping

Each Forum Topic in the Telegram group maps to one Claude session:

```rust
// All Mutex types are tokio::sync::Mutex (not std) since locks
// are held across .await points (claude::run_task, run_resume).
type SessionMap = Arc<tokio::sync::Mutex<HashMap<i64, SessionInfo>>>;

struct SessionInfo {
    claude_session_id: Option<String>,  // None if run_task didn't return one
    workdir: PathBuf,
    lock: Arc<tokio::sync::Mutex<()>>,  // per-topic lock to serialize messages
}
```

- **New topic / first message in a topic**: Create a workdir at `{config.claude_workdir}/telegram/{thread_id}/`, then call `claude::run_task(&config, &prompt, None, None, &workdir, 600)`. Store the returned `claude_session_id` (which may be `None` if CLI output didn't parse) and workdir in the map.
- **Subsequent messages in same topic**: If `claude_session_id` is `Some`, call `claude::run_resume`. If `None` (previous run didn't return a session ID), call `run_task` again (each message is independent until a session ID is obtained).
- **Messages outside a topic** (e.g., direct group message with no thread): Treated as a standalone task with no session continuity. Each standalone message gets a unique workdir at `{config.claude_workdir}/telegram/standalone/{update_id}/` to avoid concurrent workdir collisions.

**Concurrency:** Each topic has its own `Arc<Mutex<()>>` lock in `SessionInfo`. When a message arrives for a topic, the handler acquires the lock before calling Claude. If a second message arrives while the first is in-flight, it waits. For new topics (not yet in the map), the session map mutex itself serializes the initial creation, then the per-topic lock takes over.

The session map lives in memory on the plugin struct. Sessions are lost on restart, which is acceptable since Claude CLI sessions persist on disk and a new session can be started in the same topic.

### Security

Access is controlled by Telegram group membership. The bot only processes messages from the configured `chat_id` (the forum group). Messages from other chats are ignored.

### Message Flow

```
User sends message in Forum Topic
  -> getUpdates returns update with message_thread_id
  -> Validate: update.message.chat.id == configured chat_id, else skip
  -> Acquire per-topic lock (or create new SessionInfo if first message)
  -> Send typing indicator (sendChatAction)
  -> Spawn typing indicator refresh (every 4s) until Claude returns
  -> Look up thread_id in session map
     -> Found: claude::run_resume(&config, &prompt, &session_id, &workdir, 600)
     -> Not found: create workdir, claude::run_task(&config, &prompt, None, None, &workdir, 600)
       -> Store claude_session_id and workdir in map
  -> Stop typing indicator refresh
  -> Convert Claude markdown response to Telegram HTML
  -> sendMessage(chat_id, message_thread_id, html_text)
  -> Release per-topic lock
```

### Markdown to Telegram HTML Conversion

Claude returns markdown. Telegram supports a subset of HTML. The converter handles:

| Markdown | Telegram HTML |
|----------|--------------|
| `` `code` `` | `<code>code</code>` |
| ```` ```lang\nblock\n``` ```` | `<pre><code class="language-lang">block</code></pre>` |
| `**bold**` | `<b>bold</b>` |
| `*italic*` | `<i>italic</i>` |
| `- item` / `* item` | `\u2022 item` (bullet character) |
| `[text](url)` | `<a href="url">text</a>` |
| `# heading` | `<b>heading</b>` (headings become bold) |

HTML special characters (`<`, `>`, `&`) in plain text are escaped before conversion.

### Long Messages

Telegram's message limit is 4096 characters. Splitting happens on the **markdown** text before HTML conversion to avoid bisecting HTML tags. The splitter finds the nearest double-newline (paragraph break) or single newline before the 3000-character mark (conservative margin for HTML expansion -- formatting tags can double the length in dense code/bold sections). Each chunk is independently converted to HTML. After conversion, if any chunk still exceeds 4096 characters, it is hard-split at the 4096 boundary (stripping any broken tags). Each chunk is sent as a separate message.

### Error Handling

- **Claude timeout (600s)**: Send "Claude did not respond in time. Try again." to the topic.
- **Claude error**: Send the error message to the topic.
- **Polling failure**: Log the error, wait 5 seconds, retry. No message sent to Telegram.
- **sendMessage failure**: Log the error. Do not retry (avoid duplicate messages).
- **Duplicate updates**: The offset tracking mechanism prevents reprocessing. If the loop restarts after a crash, it resumes from offset 0 and may re-process the last unconfirmed update, which would create a new session (since the session map is lost). This is acceptable -- the user gets a fresh response rather than silence.

### "Thinking" Indicator

Before calling Claude, the bot sends `sendChatAction` with `action=typing`. Since Telegram auto-expires this after ~5 seconds, a background task re-sends it every 4 seconds until the Claude call completes. The refresh task is cancelled (via `JoinHandle::abort()`) once a response is received.

## Config

No new config fields required. Existing fields are sufficient:

```toml
[plugins.telegram]
enabled = true
bot_token = "123456:ABC-DEF..."
chat_id = "-1001234567890"    # forum group ID
```

## Files Changed

- `src/plugins/telegram/mod.rs` — Rewrite: add polling loop, session map, message handling. Keep existing notification event handlers. Implement `on_shutdown` to abort the polling task.
- `src/plugins/telegram/markdown.rs` — New: markdown-to-Telegram-HTML converter.
- `src/plugins/telegram/polling.rs` — New: getUpdates polling loop and message dispatch logic.

## Out of Scope

- Webhook mode (can be added later if needed)
- File/image handling (text only for now)
- Bot commands (`/new`, `/status`, etc. -- can be added incrementally)
- Persisting session map across restarts
