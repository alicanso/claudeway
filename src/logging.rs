use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use crate::models::TokenUsage;

/// Per-key JSON file logger with monthly rotating log files
pub struct KeyLogger {
    log_dir: PathBuf,
}

impl KeyLogger {
    /// Create a new KeyLogger instance
    ///
    /// Creates directories for each key_id and a _unauthorized directory
    /// under the specified log_dir.
    pub fn new(log_dir: &Path, key_ids: &[String]) -> anyhow::Result<Self> {
        // Create base log directory
        fs::create_dir_all(log_dir)?;

        // Create directories for each key_id
        for key_id in key_ids {
            let key_dir = log_dir.join(key_id);
            fs::create_dir_all(&key_dir)?;
        }

        // Create _unauthorized directory
        let unauthorized_dir = log_dir.join("_unauthorized");
        fs::create_dir_all(&unauthorized_dir)?;

        Ok(KeyLogger {
            log_dir: log_dir.to_path_buf(),
        })
    }

    /// Write a line to a key-specific log file
    ///
    /// Computes the current month string and appends the line to the corresponding
    /// monthly log file. Creates the directory and file if needed.
    /// On error, prints to stderr but does not panic or propagate.
    fn write_line(&self, key_id: &str, line: &str) {
        let month_str = Utc::now().format("%Y-%m").to_string();
        let file_path = self.log_dir.join(key_id).join(format!("{}.log", month_str));

        // Ensure directory exists
        if let Err(e) = fs::create_dir_all(file_path.parent().unwrap()) {
            eprintln!("Failed to create log directory: {}", e);
            return;
        }

        // Open file in append mode and write the line
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                if let Err(e) = writeln!(file, "{}", line) {
                    eprintln!("Failed to write to log file {}: {}", file_path.display(), e);
                }
            }
            Err(e) => {
                eprintln!("Failed to open log file {}: {}", file_path.display(), e);
            }
        }
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Log a request entry
    pub fn log_request(&self, entry: &RequestLog) {
        let key_id = entry
            .key_id
            .as_ref()
            .map(|k| k.as_str())
            .unwrap_or("_unauthorized");

        match serde_json::to_string(entry) {
            Ok(json) => self.write_line(key_id, &json),
            Err(e) => eprintln!("Failed to serialize RequestLog: {}", e),
        }
    }

    /// Log a Claude invocation entry
    pub fn log_claude_invocation(&self, entry: &ClaudeInvocationLog) {
        match serde_json::to_string(entry) {
            Ok(json) => self.write_line(&entry.key_id, &json),
            Err(e) => eprintln!("Failed to serialize ClaudeInvocationLog: {}", e),
        }
    }

    /// Log an unauthorized access entry
    pub fn log_unauthorized(&self, entry: &UnauthorizedLog) {
        match serde_json::to_string(entry) {
            Ok(json) => self.write_line("_unauthorized", &json),
            Err(e) => eprintln!("Failed to serialize UnauthorizedLog: {}", e),
        }
    }
}

/// Request log entry
#[derive(Serialize)]
pub struct RequestLog {
    pub timestamp: String,
    pub level: &'static str,
    pub key_id: Option<String>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub message: String,
}

/// Claude invocation log entry
#[derive(Serialize, Deserialize)]
pub struct ClaudeInvocationLog {
    pub timestamp: String,
    pub level: &'static str,
    pub key_id: String,
    pub session_id: String,
    pub model: Option<String>,
    #[serde(rename = "claude_exit_code")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub success: bool,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
    pub message: String,
}

/// Unauthorized access log entry
#[derive(Serialize)]
pub struct UnauthorizedLog {
    pub timestamp: String,
    pub level: &'static str,
    pub method: String,
    pub path: String,
    pub remote_addr: Option<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_log_serializes() {
        let log = RequestLog {
            timestamp: "2026-03-12T10:30:00Z".to_string(),
            level: "INFO",
            key_id: Some("key-123".to_string()),
            method: "POST".to_string(),
            path: "/api/task".to_string(),
            status: 200,
            duration_ms: 150,
            message: "Task completed successfully".to_string(),
        };

        let json = serde_json::to_string(&log).expect("Failed to serialize");
        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"level\""));
        assert!(json.contains("\"key_id\""));
        assert!(json.contains("\"method\""));
        assert!(json.contains("\"path\""));
        assert!(json.contains("\"status\""));
        assert!(json.contains("\"duration_ms\""));
        assert!(json.contains("\"message\""));
        assert!(json.contains("\"POST\""));
        assert!(json.contains("200"));
    }

    #[test]
    fn test_claude_invocation_log_serializes() {
        let tokens = TokenUsage {
            input: 100,
            output: 50,
            cache_read: 10,
            cache_write: 5,
        };

        let log = ClaudeInvocationLog {
            timestamp: "2026-03-12T10:30:00Z".to_string(),
            level: "INFO",
            key_id: "key-456".to_string(),
            session_id: "session-789".to_string(),
            model: Some("claude-3-sonnet".to_string()),
            exit_code: Some(0),
            duration_ms: 500,
            success: true,
            tokens: Some(tokens),
            cost_usd: Some(0.05),
            message: "Claude invocation completed".to_string(),
        };

        let json = serde_json::to_string(&log).expect("Failed to serialize");
        assert!(json.contains("\"claude_exit_code\""));
        assert!(json.contains("\"cost_usd\""));
        assert!(json.contains("0.05"));
    }
}
