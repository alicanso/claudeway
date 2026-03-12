use axum::Json;
use chrono::{DateTime, Utc};
use serde_json;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::models::{ModelInfo, ModelsResponse};

const CACHE_TTL_SECS: u64 = 6 * 3600; // 6 hours

struct CachedModels {
    models: Vec<ModelInfo>,
    fetched_at: DateTime<Utc>,
}

pub struct ModelsCache {
    models: RwLock<Option<CachedModels>>,
}

impl ModelsCache {
    pub fn new() -> Self {
        ModelsCache {
            models: RwLock::new(None),
        }
    }

    pub async fn get_models(&self) -> Vec<ModelInfo> {
        let read_lock = self.models.read().await;

        if let Some(cached) = read_lock.as_ref() {
            let age_secs = Utc::now()
                .signed_duration_since(cached.fetched_at)
                .num_seconds() as u64;

            if age_secs < CACHE_TTL_SECS {
                // Cache is fresh
                return cached.models.clone();
            } else {
                // Cache is stale, return stale clone (don't block)
                return cached.models.clone();
            }
        }

        // No cache, drop read lock and fetch
        drop(read_lock);

        let models = Self::fetch_models().await;

        let mut write_lock = self.models.write().await;
        *write_lock = Some(CachedModels {
            models: models.clone(),
            fetched_at: Utc::now(),
        });

        models
    }

    async fn fetch_models() -> Vec<ModelInfo> {
        // Try reading ~/.claude/settings.json
        let settings_path = dirs_next::home_dir()
            .map(|h| h.join(".claude").join("settings.json"));

        if let Some(path) = settings_path {
            match tokio::fs::read_to_string(&path).await {
                Ok(contents) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                        if let Some(available_models) = json.get("availableModels").and_then(|v| v.as_array()) {
                            let models: Vec<ModelInfo> = available_models
                                .iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .map(|id| ModelInfo {
                                    id: id.clone(),
                                    name: model_display_name(&id).to_string(),
                                })
                                .collect();

                            if !models.is_empty() {
                                return models;
                            }
                        }
                    }
                }
                Err(_) => {
                    // Fall through to default_models()
                }
            }
        }

        default_models()
    }
}

fn default_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-haiku-4-5-20251001".to_string(),
            name: "Claude Haiku 4.5".to_string(),
        },
        ModelInfo {
            id: "claude-sonnet-4-6".to_string(),
            name: "Claude Sonnet 4.6".to_string(),
        },
        ModelInfo {
            id: "claude-opus-4-6".to_string(),
            name: "Claude Opus 4.6".to_string(),
        },
    ]
}

fn model_display_name(id: &str) -> &str {
    if id.contains("haiku") {
        "Claude Haiku 4.5"
    } else if id.contains("sonnet") {
        "Claude Sonnet 4.6"
    } else if id.contains("opus") {
        "Claude Opus 4.6"
    } else {
        id
    }
}

pub async fn list_models(cache: Arc<ModelsCache>) -> Json<ModelsResponse> {
    let models = cache.get_models().await;
    Json(ModelsResponse { models })
}
