use clap::Parser;
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Claudeway — HTTP gateway for the Claude CLI
#[derive(Parser, Debug)]
#[command(name = "claudeway", version, about)]
struct Cli {
    /// API keys as key_id:key_value, comma-separated
    #[arg(long, env = "WRAPPER_KEYS")]
    keys: Option<String>,

    /// Path to claude CLI binary
    #[arg(long, env = "CLAUDE_BIN", default_value = "claude")]
    claude_bin: String,

    /// Base directory for session workdirs
    #[arg(long, env = "CLAUDE_WORKDIR", default_value = "/tmp/claude-tasks")]
    workdir: String,

    /// Base directory for per-key log files
    #[arg(long, env = "LOG_DIR", default_value = "./logs")]
    log_dir: String,

    /// HTTP listen host
    #[arg(long, env = "HOST", default_value = "0.0.0.0")]
    host: String,

    /// HTTP listen port
    #[arg(short, long, env = "PORT", default_value_t = 3000)]
    port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Path to config file (claudeway.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Disable specific plugins (overrides config file)
    #[arg(long, value_delimiter = ',')]
    disable_plugin: Vec<String>,

    /// Enable specific plugins (overrides config file)
    #[arg(long, value_delimiter = ',')]
    enable_plugin: Vec<String>,

    /// Skip interactive prompts (use defaults)
    #[arg(long, short = 'f')]
    force: bool,

    /// Disable --dangerously-skip-permissions (require Claude to ask for permission)
    #[arg(long, env = "NO_PERMISSIONS_BYPASS")]
    no_permissions_bypass: bool,
}

pub struct Config {
    pub api_keys: HashMap<String, String>,
    pub admin_key_id: String,
    pub claude_bin: String,
    pub claude_workdir: String,
    pub log_dir: String,
    pub host: String,
    pub port: u16,
    pub log_level: String,
    /// If a key was auto-generated, this holds the secret so we can print it at startup.
    pub generated_key: Option<String>,
    pub config_path: Option<PathBuf>,
    pub disabled_plugins: Vec<String>,
    pub enabled_plugins: Vec<String>,
    pub force: bool,
    pub bypass_permissions: bool,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let cli = Cli::parse();

        let (api_keys, admin_key_id, generated_key) = match cli.keys {
            Some(raw) => {
                let (keys, admin) = Self::parse_keys_with_admin(&raw)?;
                (keys, admin, None)
            }
            None => {
                let secret = generate_secret();
                let mut map = HashMap::new();
                map.insert(secret.clone(), "default".to_string());
                (map, "default".to_string(), Some(secret))
            }
        };

        Ok(Self {
            api_keys,
            admin_key_id,
            claude_bin: cli.claude_bin,
            claude_workdir: cli.workdir,
            log_dir: cli.log_dir,
            host: cli.host,
            port: cli.port,
            log_level: cli.log_level,
            generated_key,
            config_path: cli.config,
            disabled_plugins: cli.disable_plugin,
            enabled_plugins: cli.enable_plugin,
            force: cli.force,
            bypass_permissions: !cli.no_permissions_bypass,
        })
    }

    pub fn parse_keys(raw: &str) -> anyhow::Result<HashMap<String, String>> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
        }

        let mut map = HashMap::new();
        for entry in trimmed.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let parts: Vec<&str> = entry.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!(
                    "Invalid key format: expected 'key_id:key_value', got '{entry}'"
                ));
            }
            let id = parts[0].trim();
            let value = parts[1].trim();
            if id.is_empty() {
                return Err(anyhow::anyhow!("Key ID cannot be empty"));
            }
            if value.is_empty() {
                return Err(anyhow::anyhow!("Key value cannot be empty"));
            }
            map.insert(value.to_string(), id.to_string());
        }

        if map.is_empty() {
            return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
        }

        Ok(map)
    }

    pub fn parse_keys_with_admin(raw: &str) -> anyhow::Result<(HashMap<String, String>, String)> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
        }
        let mut map = HashMap::new();
        let mut admin_key_id: Option<String> = None;
        for entry in trimmed.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let parts: Vec<&str> = entry.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!(
                    "Invalid key format: expected 'key_id:key_value', got '{entry}'"
                ));
            }
            let id = parts[0].trim();
            let value = parts[1].trim();
            if id.is_empty() {
                return Err(anyhow::anyhow!("Key ID cannot be empty"));
            }
            if value.is_empty() {
                return Err(anyhow::anyhow!("Key value cannot be empty"));
            }
            if admin_key_id.is_none() {
                admin_key_id = Some(id.to_string());
            }
            map.insert(value.to_string(), id.to_string());
        }
        if map.is_empty() {
            return Err(anyhow::anyhow!("WRAPPER_KEYS cannot be empty"));
        }
        Ok((map, admin_key_id.unwrap()))
    }

    pub fn key_ids(&self) -> Vec<&String> {
        self.api_keys.values().collect()
    }
}

fn generate_secret() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("sk-{hex}")
}

/// Config loaded from claudeway.toml file
#[derive(serde::Deserialize, Default)]
pub struct PluginConfig {
    #[serde(default)]
    pub plugins: HashMap<String, toml_crate::Value>,
}

impl PluginConfig {
    /// Load from file path, or return default if no config file found
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let config_path = match path {
            Some(p) => {
                if p.exists() {
                    Some(p.to_path_buf())
                } else {
                    return Err(anyhow::anyhow!("Config file not found: {}", p.display()));
                }
            }
            None => {
                let default_path = PathBuf::from("claudeway.toml");
                if default_path.exists() {
                    Some(default_path)
                } else {
                    None
                }
            }
        };

        match config_path {
            Some(path) => {
                let content = std::fs::read_to_string(&path)?;
                let config: PluginConfig = toml_crate::from_str(&content)?;
                Ok(config)
            }
            None => Ok(PluginConfig::default()),
        }
    }

    pub fn is_plugin_enabled(
        &self,
        name: &str,
        disabled_plugins: &[String],
        enabled_plugins: &[String],
    ) -> bool {
        // CLI --disable-plugin always wins
        if disabled_plugins.iter().any(|d| d == name) {
            return false;
        }
        // CLI --enable-plugin overrides config
        if enabled_plugins.iter().any(|e| e == name) {
            return true;
        }
        // Config file, default = false (disabled)
        self.plugins
            .get(name)
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn get_str(&self, plugin: &str, key: &str) -> Option<String> {
        self.plugins
            .get(plugin)
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_key() {
        let keys = Config::parse_keys("myid:myvalue").unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys.get("myvalue").unwrap(), "myid");
    }

    #[test]
    fn test_multiple_keys() {
        let keys = Config::parse_keys("id1:val1,id2:val2").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.get("val1").unwrap(), "id1");
        assert_eq!(keys.get("val2").unwrap(), "id2");
    }

    #[test]
    fn test_empty_fails() {
        assert!(Config::parse_keys("").is_err());
        assert!(Config::parse_keys("   ").is_err());
    }

    #[test]
    fn test_no_colon_fails() {
        assert!(Config::parse_keys("nocolonhere").is_err());
    }

    #[test]
    fn test_empty_id_fails() {
        assert!(Config::parse_keys(":value").is_err());
    }

    #[test]
    fn test_empty_value_fails() {
        assert!(Config::parse_keys("id:").is_err());
    }

    #[test]
    fn test_parse_keys_returns_admin_key_id() {
        let (keys, admin_key_id) = Config::parse_keys_with_admin("admin:sk-001,ci:sk-002").unwrap();
        assert_eq!(admin_key_id, "admin");
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_whitespace_trimming() {
        let keys = Config::parse_keys("  id1 : val1 , id2 : val2  ").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.get("val1").unwrap(), "id1");
        assert_eq!(keys.get("val2").unwrap(), "id2");
    }

    #[test]
    fn test_plugin_config_default_disabled() {
        let config = PluginConfig::default();
        assert!(!config.is_plugin_enabled("dashboard", &[], &[]));
    }

    #[test]
    fn test_plugin_config_enabled_by_cli() {
        let config = PluginConfig::default();
        let enabled = vec!["dashboard".to_string()];
        assert!(config.is_plugin_enabled("dashboard", &[], &enabled));
    }

    #[test]
    fn test_plugin_config_disabled_by_cli_wins() {
        let config = PluginConfig::default();
        let disabled = vec!["dashboard".to_string()];
        let enabled = vec!["dashboard".to_string()];
        assert!(!config.is_plugin_enabled("dashboard", &disabled, &enabled));
    }

    #[test]
    fn test_plugin_config_disabled_in_file() {
        let toml_str = r#"
            [plugins.dashboard]
            enabled = false
        "#;
        let config: PluginConfig = toml_crate::from_str(toml_str).unwrap();
        assert!(!config.is_plugin_enabled("dashboard", &[], &[]));
    }

    #[test]
    fn test_plugin_config_enabled_in_file() {
        let toml_str = r#"
            [plugins.swagger]
            enabled = true
        "#;
        let config: PluginConfig = toml_crate::from_str(toml_str).unwrap();
        assert!(config.is_plugin_enabled("swagger", &[], &[]));
    }
}
