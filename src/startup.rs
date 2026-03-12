use std::io::{self, Write};
use std::path::Path;

use crate::config::Config;

/// Available plugins for interactive setup
const AVAILABLE_PLUGINS: &[(&str, &str)] = &[
    ("dashboard", "Admin dashboard with sessions, logs, costs"),
    ("swagger", "OpenAPI 3.1 spec + Swagger UI at /docs"),
    ("telegram", "Forward events to a Telegram chat"),
    ("slack", "Post events to a Slack channel"),
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

    eprintln!();
    eprintln!("  \x1b[1;36m┌─────────────────────────────────────────┐\x1b[0m");
    eprintln!("  \x1b[1;36m│\x1b[0m   \x1b[1mWelcome to Claudeway!\x1b[0m                 \x1b[1;36m│\x1b[0m");
    eprintln!("  \x1b[1;36m│\x1b[0m   First-time setup — pick your plugins  \x1b[1;36m│\x1b[0m");
    eprintln!("  \x1b[1;36m└─────────────────────────────────────────┘\x1b[0m");
    eprintln!();

    let mut selected = Vec::new();

    for (i, (name, desc)) in AVAILABLE_PLUGINS.iter().enumerate() {
        eprint!("  \x1b[1m[{}]\x1b[0m {} — {}", i + 1, name, desc);
        eprint!(" \x1b[2m(y/N)\x1b[0m ");
        io::stderr().flush().ok();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let answer = input.trim().to_lowercase();
            if answer == "y" || answer == "yes" {
                selected.push(name.to_string());
            }
        }
    }

    // Write config file
    let mut toml_content = String::new();
    for (name, _) in AVAILABLE_PLUGINS {
        let enabled = selected.contains(&name.to_string());
        toml_content.push_str(&format!("[plugins.{}]\nenabled = {}\n\n", name, enabled));
    }

    if std::fs::write(config_path, &toml_content).is_ok() {
        eprintln!();
        eprintln!(
            "  \x1b[32m✓\x1b[0m Saved to \x1b[1m{}\x1b[0m",
            config_path.display()
        );
    }

    Some(selected)
}

/// Print the startup banner with server info.
pub fn print_banner(config: &Config, enabled_plugins: &[String]) {
    let version = env!("CARGO_PKG_VERSION");
    let url = format!("http://localhost:{}", config.port);

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

    eprintln!();
    eprintln!("  \x1b[1;35m╔══════════════════════════════════════════════╗\x1b[0m");
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;37m⚡ Claudeway v{:<30}\x1b[0m \x1b[1;35m║\x1b[0m", version);
    eprintln!("  \x1b[1;35m╠══════════════════════════════════════════════╣\x1b[0m");
    eprintln!("  \x1b[1;35m║\x1b[0m                                              \x1b[1;35m║\x1b[0m");
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;32m→\x1b[0m URL       \x1b[1m{:<33}\x1b[0m\x1b[1;35m║\x1b[0m", url);
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;33m→\x1b[0m Auth      {:<33}\x1b[1;35m║\x1b[0m", key_display);
    eprintln!("  \x1b[1;35m║\x1b[0m  \x1b[1;36m→\x1b[0m Plugins   {:<33}\x1b[1;35m║\x1b[0m", plugins_display);
    eprintln!("  \x1b[1;35m║\x1b[0m                                              \x1b[1;35m║\x1b[0m");
    eprintln!("  \x1b[1;35m╚══════════════════════════════════════════════╝\x1b[0m");
    eprintln!();

    if config.generated_key.is_some() {
        eprintln!("  \x1b[2mTip: set your own keys with --keys or WRAPPER_KEYS env var.\x1b[0m");
        eprintln!();
    }
}
