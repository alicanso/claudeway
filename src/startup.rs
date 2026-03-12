use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal;

use crate::config::Config;

/// Plugin definition for interactive setup
struct PluginDef {
    name: &'static str,
    description: &'static str,
    /// Config fields to prompt for when selected: (key, label, required)
    config_fields: &'static [(&'static str, &'static str, bool)],
}

/// Available plugins for interactive setup
const PLUGINS: &[PluginDef] = &[
    PluginDef {
        name: "dashboard",
        description: "Admin dashboard with sessions, logs, costs",
        config_fields: &[],
    },
    PluginDef {
        name: "swagger",
        description: "OpenAPI 3.1 spec + Swagger UI at /docs",
        config_fields: &[],
    },
    PluginDef {
        name: "cloudflare_tunnel",
        description: "Expose server via Cloudflare Tunnel",
        config_fields: &[
            ("tunnel_token", "Tunnel token (leave empty for quick tunnel)", false),
        ],
    },
    PluginDef {
        name: "telegram",
        description: "Forward events to a Telegram chat",
        config_fields: &[
            ("bot_token", "Bot token (from @BotFather)", true),
        ],
    },
    PluginDef {
        name: "slack",
        description: "Post events to a Slack channel",
        config_fields: &[
            ("webhook_url", "Webhook URL", true),
        ],
    },
];

/// Run interactive plugin selection on first startup (no config file).
/// Returns the list of enabled plugin names, or None if skipped.
pub fn interactive_setup(config: &Config) -> Option<Vec<String>> {
    // Skip if --force, or if a config file already exists
    if config.force {
        return None;
    }

    let config_path = config
        .config_path
        .as_deref()
        .unwrap_or(Path::new("claudeway.toml"));

    if config_path.exists() {
        return None;
    }

    // Also skip if any --enable-plugin was passed
    if !config.enabled_plugins.is_empty() {
        return None;
    }

    // Skip if not a terminal (piped input, CI, etc.)
    if !atty_stderr() {
        return None;
    }

    eprintln!();
    eprintln!("  \x1b[1;36m┌─────────────────────────────────────────┐\x1b[0m");
    eprintln!("  \x1b[1;36m│\x1b[0m   \x1b[1mWelcome to Claudeway!\x1b[0m                 \x1b[1;36m│\x1b[0m");
    eprintln!("  \x1b[1;36m│\x1b[0m   First-time setup — pick your plugins  \x1b[1;36m│\x1b[0m");
    eprintln!("  \x1b[1;36m└─────────────────────────────────────────┘\x1b[0m");
    eprintln!();
    eprintln!("  \x1b[2mUse ↑/↓ to navigate, Space to toggle, Enter to confirm\x1b[0m");
    eprintln!();

    // Phase 1: Checkbox selection
    let selected = match checkbox_select() {
        Some(s) => s,
        None => return Some(Vec::new()),
    };

    // Phase 2: Per-plugin config prompts
    // plugin_name -> { key -> value }
    let mut plugin_configs: HashMap<&str, HashMap<&str, String>> = HashMap::new();

    for &idx in &selected {
        let plugin = &PLUGINS[idx];
        if plugin.config_fields.is_empty() {
            continue;
        }

        eprintln!();
        eprintln!(
            "  \x1b[1;36m─── {} configuration ───\x1b[0m",
            plugin.name
        );
        eprintln!();

        let mut fields = HashMap::new();
        let mut skip = false;

        for &(key, label, required) in plugin.config_fields {
            eprint!("  \x1b[1m{}\x1b[0m: ", label);
            io::stderr().flush().ok();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let value = input.trim().to_string();
                if value.is_empty() && required {
                    eprintln!(
                        "  \x1b[33m⚠\x1b[0m Skipping {} (required field empty)",
                        plugin.name
                    );
                    skip = true;
                    break;
                }
                if !value.is_empty() {
                    fields.insert(key, value);
                }
            }
        }

        if !skip {
            // Special handling: auto-detect Telegram chat_id
            if plugin.name == "telegram" {
                if let Some(bot_token) = fields.get("bot_token") {
                    match detect_telegram_chat_id(bot_token) {
                        Some(chat_id) => {
                            fields.insert("chat_id", chat_id);
                        }
                        None => {
                            eprintln!(
                                "  \x1b[33m⚠\x1b[0m Could not detect chat ID, skipping telegram"
                            );
                            continue;
                        }
                    }
                }
            }
            plugin_configs.insert(plugin.name, fields);
        }
    }

    // Build selected plugin names
    let selected_names: Vec<String> = selected
        .iter()
        .map(|&idx| PLUGINS[idx].name.to_string())
        .filter(|name| {
            let plugin = PLUGINS.iter().find(|p| p.name == name).unwrap();
            // If it has required config fields, it must have been configured
            if plugin.config_fields.iter().any(|&(_, _, req)| req) {
                plugin_configs.contains_key(name.as_str())
            } else {
                true
            }
        })
        .collect();

    // Write config file
    let mut toml_content = String::new();
    for plugin in PLUGINS {
        let enabled = selected_names.contains(&plugin.name.to_string());
        toml_content.push_str(&format!("[plugins.{}]\nenabled = {}\n", plugin.name, enabled));

        if let Some(fields) = plugin_configs.get(plugin.name) {
            for (key, value) in fields {
                toml_content.push_str(&format!("{} = \"{}\"\n", key, value));
            }
        }

        toml_content.push('\n');
    }

    if std::fs::write(config_path, &toml_content).is_ok() {
        eprintln!();
        eprintln!(
            "  \x1b[32m✓\x1b[0m Saved to \x1b[1m{}\x1b[0m",
            config_path.display()
        );
    }

    Some(selected_names)
}

/// Interactive checkbox selection using crossterm raw mode.
/// Returns indices of selected plugins, or None on error.
fn checkbox_select() -> Option<Vec<usize>> {
    let count = PLUGINS.len();
    let mut checked = vec![false; count];
    let mut cursor = 0;

    // Draw initial list
    render_checkboxes(&checked, cursor);

    // Enter raw mode for key-by-key input
    if terminal::enable_raw_mode().is_err() {
        // Fallback to line-based input if raw mode fails
        return checkbox_select_fallback();
    }

    let result = loop {
        if let Ok(Event::Key(key)) = event::read() {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if cursor > 0 {
                        cursor -= 1;
                    } else {
                        cursor = count - 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if cursor < count - 1 {
                        cursor += 1;
                    } else {
                        cursor = 0;
                    }
                }
                KeyCode::Char(' ') => {
                    checked[cursor] = !checked[cursor];
                }
                KeyCode::Enter => {
                    break checked;
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    let _ = terminal::disable_raw_mode();
                    // Clear the checkbox area
                    clear_lines(count);
                    eprintln!("  \x1b[2mSkipped plugin selection.\x1b[0m");
                    return Some(Vec::new());
                }
                KeyCode::Char('a') => {
                    // Toggle all
                    let all_checked = checked.iter().all(|&c| c);
                    for c in &mut checked {
                        *c = !all_checked;
                    }
                }
                _ => {}
            }

            // Redraw
            clear_lines(count);
            render_checkboxes(&checked, cursor);
        }
    };

    let _ = terminal::disable_raw_mode();

    // Clear checkbox area and print summary
    clear_lines(count);

    let selected: Vec<usize> = result
        .iter()
        .enumerate()
        .filter(|(_, c)| **c)
        .map(|(i, _)| i)
        .collect();

    if selected.is_empty() {
        eprintln!("  \x1b[2mNo plugins selected.\x1b[0m");
    } else {
        let names: Vec<&str> = selected.iter().map(|&i| PLUGINS[i].name).collect();
        eprintln!(
            "  \x1b[32m✓\x1b[0m Selected: \x1b[1m{}\x1b[0m",
            names.join(", ")
        );
    }

    Some(selected)
}

/// Fallback checkbox selection for non-TTY environments
fn checkbox_select_fallback() -> Option<Vec<usize>> {
    let mut selected = Vec::new();

    for (i, plugin) in PLUGINS.iter().enumerate() {
        eprint!(
            "  \x1b[1m[{}]\x1b[0m {} — {}",
            i + 1,
            plugin.name,
            plugin.description
        );
        eprint!(" \x1b[2m(y/N)\x1b[0m ");
        io::stderr().flush().ok();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let answer = input.trim().to_lowercase();
            if answer == "y" || answer == "yes" {
                selected.push(i);
            }
        }
    }

    Some(selected)
}

/// Auto-detect Telegram chat ID by polling /getUpdates.
/// Asks user to send a message to the bot, then fetches the chat_id.
fn detect_telegram_chat_id(bot_token: &str) -> Option<String> {
    eprintln!();
    eprintln!(
        "  \x1b[1;33m→\x1b[0m Send any message to your bot in Telegram, then press \x1b[1mEnter\x1b[0m"
    );
    eprint!("    \x1b[2mWaiting...\x1b[0m ");
    io::stderr().flush().ok();

    // Wait for user to press Enter
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);

    // Call Telegram getUpdates API (blocking)
    let url = format!(
        "https://api.telegram.org/bot{}/getUpdates?limit=1&offset=-1",
        bot_token
    );

    // Run blocking HTTP call on a separate thread to avoid nesting
    // a Tokio runtime inside the #[tokio::main] async context.
    let body: serde_json::Value = match std::thread::spawn(move || -> Result<serde_json::Value, String> {
        let response = reqwest::blocking::get(&url)
            .map_err(|e| format!("Failed to reach Telegram API: {e}"))?;
        response
            .json()
            .map_err(|e| format!("Invalid response from Telegram: {e}"))
    })
    .join()
    .unwrap()
    {
        Ok(j) => j,
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m {e}");
            return None;
        }
    };

    if let Some(chat_id) = body["result"][0]["message"]["chat"]["id"].as_i64() {
        let chat_id_str = chat_id.to_string();
        eprintln!(
            "  \x1b[32m✓\x1b[0m Detected chat ID: \x1b[1m{}\x1b[0m",
            chat_id_str
        );
        Some(chat_id_str)
    } else {
        eprintln!("  \x1b[31m✗\x1b[0m No messages found. Make sure you sent a message to the bot.");
        None
    }
}

/// Render the checkbox list
fn render_checkboxes(checked: &[bool], cursor: usize) {
    for (i, plugin) in PLUGINS.iter().enumerate() {
        let marker = if checked[i] {
            "\x1b[1;32m✓\x1b[0m"
        } else {
            " "
        };
        let pointer = if i == cursor {
            "\x1b[1;36m❯\x1b[0m"
        } else {
            " "
        };
        let name_style = if i == cursor {
            "\x1b[1m"
        } else {
            ""
        };
        eprint!(
            "  {} [{}] {}{}\x1b[0m \x1b[2m— {}\x1b[0m\r\n",
            pointer, marker, name_style, plugin.name, plugin.description
        );
    }
    io::stderr().flush().ok();
}

/// Move cursor up N lines and clear them
fn clear_lines(n: usize) {
    for _ in 0..n {
        eprint!("\x1b[A\x1b[2K");
    }
    io::stderr().flush().ok();
}

/// Check if stderr is a terminal (simple heuristic)
fn atty_stderr() -> bool {
    // If we can enable/disable raw mode, it's a real terminal
    if terminal::enable_raw_mode().is_ok() {
        let _ = terminal::disable_raw_mode();
        return true;
    }
    false
}

/// Strip ANSI escape sequences to get the visible length of a string.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

/// Print the startup banner with server info.
pub fn print_banner(config: &Config, enabled_plugins: &[String]) {
    let version = env!("CARGO_PKG_VERSION");
    let url = format!("http://localhost:{}", config.port);
    let health_url = format!("{}/health", url);

    let key_display = if let Some(ref key) = config.generated_key {
        format!("{} \x1b[2m(auto-generated)\x1b[0m", key)
    } else {
        let count = config.api_keys.len();
        let ids: Vec<&String> = config.api_keys.values().collect();
        if count <= 3 {
            format!("{} key(s): {}", count, ids.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
        } else {
            format!("{count} key(s) loaded")
        }
    };

    let plugins_display = if enabled_plugins.is_empty() {
        "\x1b[2mnone\x1b[0m".to_string()
    } else {
        enabled_plugins.join(", ")
    };

    let label_width = 10; // "URL       ", "Dashboard ", etc.
    let prefix = 2; // "→ " before label
    let padding = 2; // spaces on each side inside the box

    let title = format!("⚡ Claudeway v{}", version);

    // Build rows dynamically: (label, value, color)
    let mut rows: Vec<(&str, String, &str)> = vec![
        ("URL", url.clone(), "\x1b[1;32m"),
        ("Health", health_url, "\x1b[1;34m"),
    ];
    if enabled_plugins.iter().any(|p| p == "dashboard") {
        rows.push(("Dashboard", format!("{}/dashboard", url), "\x1b[1;35m"));
    }
    if enabled_plugins.iter().any(|p| p == "swagger") {
        rows.push(("Docs", format!("{}/docs", url), "\x1b[1;35m"));
    }
    let key_plain = format!("Auth      {}", key_display);
    let plugins_plain = format!("Plugins   {}", plugins_display);

    // Calculate content width from the widest row
    let mut max_w = title.len();
    for (_label, value, _color) in &rows {
        let w = prefix + label_width + value.len();
        if w > max_w { max_w = w; }
    }
    let key_vis = prefix + visible_len(&key_plain);
    if key_vis > max_w { max_w = key_vis; }
    let plugins_vis = prefix + visible_len(&plugins_plain);
    if plugins_vis > max_w { max_w = plugins_vis; }

    let inner = max_w + padding * 2; // total chars between ║ and ║
    let bar = "═".repeat(inner);
    let empty: String = " ".repeat(inner);

    eprintln!();
    eprintln!("  \x1b[1;35m╔{bar}╗\x1b[0m");
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;37m{:<width$}\x1b[0m  \x1b[1;35m║\x1b[0m", title, width = inner - padding * 2);
    eprintln!("  \x1b[1;35m╠{bar}╣\x1b[0m");
    eprintln!("  \x1b[1;35m║\x1b[0m{empty}\x1b[1;35m║\x1b[0m");

    for (label, value, color) in &rows {
        let content = format!("{color}→\x1b[0m {:<lw$}\x1b[1m{}\x1b[0m", label, value, lw = label_width);
        let vis = prefix + label_width + value.len();
        let pad = inner - padding - vis;
        eprintln!("  \x1b[1;35m║\x1b[0m  {content}{:>pad$}  \x1b[1;35m║\x1b[0m", "", pad = pad);
    }

    // Auth row
    let auth_pad = inner - padding - key_vis;
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;33m→\x1b[0m {key_plain}{:>pad$}  \x1b[1;35m║\x1b[0m", "", pad = auth_pad);

    // Plugins row
    let plugins_pad = inner - padding - plugins_vis;
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;36m→\x1b[0m {plugins_plain}{:>pad$}  \x1b[1;35m║\x1b[0m", "", pad = plugins_pad);

    eprintln!("  \x1b[1;35m║\x1b[0m{empty}\x1b[1;35m║\x1b[0m");
    eprintln!("  \x1b[1;35m╚{bar}╝\x1b[0m");
    eprintln!();

    if config.generated_key.is_some() {
        eprintln!("  \x1b[2mTip: set your own keys with --keys or WRAPPER_KEYS env var.\x1b[0m");
        eprintln!();
    }
}
