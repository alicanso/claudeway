use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// --- Login ---
#[derive(Debug, Deserialize)]
pub struct AdminLoginRequest {
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct AdminLoginResponse {
    pub success: bool,
}

// --- Overview ---
#[derive(Debug, Serialize)]
pub struct OverviewResponse {
    pub uptime_secs: u64,
    pub total_requests: u64,
    pub active_sessions: u64,
    pub total_cost_usd: f64,
    pub models_breakdown: Vec<ModelBreakdown>,
}

#[derive(Debug, Serialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub request_count: u64,
    pub cost_usd: f64,
}

// --- Sessions ---
#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    pub page: Option<u64>,
    pub limit: Option<u64>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionsListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: u64,
    pub page: u64,
    pub limit: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u32,
    pub cost_usd: f64,
    pub key_id: String,
}

#[derive(Debug, Serialize)]
pub struct SessionDetailResponse {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub task_count: u32,
    pub cost_usd: f64,
    pub key_id: String,
    pub tokens: crate::models::TokenUsage,
    pub workdir: String,
}

// --- Logs ---
#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub key_id: Option<String>,
    pub date: Option<String>,
    pub after: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub entries: Vec<serde_json::Value>,
    pub total: u64,
}

// --- Keys ---
#[derive(Debug, Serialize)]
pub struct KeysResponse {
    pub keys: Vec<KeyStats>,
}

#[derive(Debug, Serialize)]
pub struct KeyStats {
    pub key_id: String,
    pub total_requests: u64,
    pub total_cost_usd: f64,
}

// --- Costs ---
#[derive(Debug, Deserialize)]
pub struct CostsQuery {
    pub group_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CostsResponse {
    pub data: Vec<CostEntry>,
}

#[derive(Debug, Serialize)]
pub struct CostEntry {
    pub period: String,
    pub cost_usd: f64,
    pub request_count: u64,
    pub by_model: Vec<ModelBreakdown>,
    pub by_key: Vec<KeyCost>,
}

#[derive(Debug, Serialize)]
pub struct KeyCost {
    pub key_id: String,
    pub cost_usd: f64,
}
