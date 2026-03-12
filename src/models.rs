use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// --- Shared ---

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenUsage {
    pub fn accumulate(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PermissionDenial {
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: serde_json::Value,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApprovePermissionsRequest {
    pub tool_use_ids: Vec<String>,
}

// --- Health ---

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
}

// --- Models ---

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

// --- Task ---

#[derive(Debug, Deserialize, ToSchema)]
pub struct TaskRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TaskResponse {
    pub session_id: String,
    pub result: Option<String>,
    pub success: bool,
    pub duration_ms: u64,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub permission_denials: Vec<PermissionDenial>,
}

// --- Session ---

#[derive(Debug, Deserialize, ToSchema)]
pub struct SessionStartRequest {
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionStartResponse {
    pub session_id: String,
    pub workdir: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SessionContinueRequest {
    pub prompt: String,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionInfoResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u64,
    pub workdir: String,
    pub tokens: TokenUsage,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteSessionResponse {
    pub deleted: bool,
    pub session_id: String,
}

// --- Claude CLI parsing ---

#[derive(Debug, Deserialize)]
pub struct ClaudeCliOutput {
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    pub session_id: Option<String>,
    pub result: Option<String>,
    #[serde(rename = "costUSD")]
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    #[serde(rename = "isError")]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub permission_denials: Vec<PermissionDenial>,
}

#[derive(Debug, Deserialize)]
pub struct SessionJSONLEntry {
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    pub message: Option<SessionMessage>,
}

#[derive(Debug, Deserialize)]
pub struct SessionMessage {
    pub usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize)]
pub struct MessageUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_accumulate() {
        let mut a = TokenUsage {
            input: 10,
            output: 20,
            cache_read: 5,
            cache_write: 3,
        };
        let b = TokenUsage {
            input: 5,
            output: 10,
            cache_read: 2,
            cache_write: 1,
        };
        a.accumulate(&b);
        assert_eq!(a.input, 15);
        assert_eq!(a.output, 30);
        assert_eq!(a.cache_read, 7);
        assert_eq!(a.cache_write, 4);
    }

    #[test]
    fn test_parse_claude_cli_output() {
        let json = r#"{
            "type": "result",
            "session_id": "abc-123",
            "result": "Hello world",
            "costUSD": 0.05,
            "duration_ms": 1200,
            "isError": false
        }"#;
        let output: ClaudeCliOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.r#type.as_deref(), Some("result"));
        assert_eq!(output.session_id.as_deref(), Some("abc-123"));
        assert_eq!(output.result.as_deref(), Some("Hello world"));
        assert_eq!(output.cost_usd, Some(0.05));
        assert_eq!(output.duration_ms, Some(1200));
        assert_eq!(output.is_error, Some(false));
    }

    #[test]
    fn test_parse_session_jsonl_entry() {
        let json = r#"{
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 50,
                    "cache_creation_input_tokens": 10,
                    "cache_read_input_tokens": 20
                }
            }
        }"#;
        let entry: SessionJSONLEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.r#type.as_deref(), Some("assistant"));
        let message = entry.message.unwrap();
        let usage = message.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.cache_creation_input_tokens, Some(10));
        assert_eq!(usage.cache_read_input_tokens, Some(20));
    }
}
