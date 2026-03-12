use std::collections::HashMap;
use std::env;

pub struct Config {
    pub api_keys: HashMap<String, String>,
    pub claude_bin: String,
    pub claude_workdir: String,
    pub log_dir: String,
    pub port: u16,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let keys_raw =
            env::var("WRAPPER_KEYS").map_err(|_| anyhow::anyhow!("WRAPPER_KEYS is required"))?;
        let api_keys = Self::parse_keys(&keys_raw)?;

        let claude_bin = env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());

        let claude_workdir =
            env::var("CLAUDE_WORKDIR").unwrap_or_else(|_| "/tmp/claude-tasks".to_string());

        let log_dir = env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());

        let port: u16 = env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .map_err(|_| anyhow::anyhow!("PORT must be a valid u16"))?;

        let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

        Ok(Self {
            api_keys,
            claude_bin,
            claude_workdir,
            log_dir,
            port,
            log_level,
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

    pub fn key_ids(&self) -> Vec<&String> {
        self.api_keys.values().collect()
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
    fn test_whitespace_trimming() {
        let keys = Config::parse_keys("  id1 : val1 , id2 : val2  ").unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys.get("val1").unwrap(), "id1");
        assert_eq!(keys.get("val2").unwrap(), "id2");
    }
}
