use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::admin_models::{CostEntry, KeyCost, ModelBreakdown};

/// Parse all log files and return per-model breakdown (request count + cost)
pub fn get_models_breakdown(log_dir: &Path) -> Vec<ModelBreakdown> {
    let mut model_map: HashMap<String, (u64, f64)> = HashMap::new();
    walk_log_entries(log_dir, |value, _key_id| {
        let cost = value
            .get("cost_usd")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        if cost > 0.0 {
            let entry = model_map.entry(model).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += cost;
        }
    });
    model_map
        .into_iter()
        .map(|(model, (count, cost))| ModelBreakdown {
            model,
            request_count: count,
            cost_usd: cost,
        })
        .collect()
}

/// Aggregate costs by period (daily/weekly/monthly)
pub fn aggregate_costs(log_dir: &Path, group_by: &str) -> Vec<CostEntry> {
    let mut cost_map: BTreeMap<String, (f64, u64, HashMap<String, (u64, f64)>, HashMap<String, f64>)> =
        BTreeMap::new();
    walk_log_entries(log_dir, |value, key_id| {
        let cost = value
            .get("cost_usd")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if cost == 0.0 {
            return;
        }
        let timestamp = value
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let period = match group_by {
            "weekly" => {
                if timestamp.len() >= 10 {
                    if let Ok(date) =
                        chrono::NaiveDate::parse_from_str(&timestamp[..10], "%Y-%m-%d")
                    {
                        use chrono::Datelike;
                        format!(
                            "{}-W{:02}",
                            date.iso_week().year(),
                            date.iso_week().week()
                        )
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
            "monthly" => {
                if timestamp.len() >= 7 {
                    timestamp[..7].to_string()
                } else {
                    return;
                }
            }
            _ => {
                if timestamp.len() >= 10 {
                    timestamp[..10].to_string()
                } else {
                    return;
                }
            }
        };
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let entry = cost_map
            .entry(period)
            .or_insert_with(|| (0.0, 0, HashMap::new(), HashMap::new()));
        entry.0 += cost;
        entry.1 += 1;
        let model_entry = entry.2.entry(model).or_insert((0, 0.0));
        model_entry.0 += 1;
        model_entry.1 += cost;
        *entry.3.entry(key_id.to_string()).or_insert(0.0) += cost;
    });
    cost_map
        .into_iter()
        .map(|(period, (cost, count, by_model, by_key))| CostEntry {
            period,
            cost_usd: cost,
            request_count: count,
            by_model: by_model
                .into_iter()
                .map(|(model, (req_count, cost))| ModelBreakdown {
                    model,
                    request_count: req_count,
                    cost_usd: cost,
                })
                .collect(),
            by_key: by_key
                .into_iter()
                .map(|(key_id, cost)| KeyCost { key_id, cost_usd: cost })
                .collect(),
        })
        .collect()
}

/// Get per-key stats (total requests and cost) from log files
pub fn get_keys_stats(log_dir: &Path, key_ids: &[String]) -> Vec<crate::admin_models::KeyStats> {
    let mut stats: HashMap<String, (u64, f64)> = HashMap::new();
    for key_id in key_ids {
        stats.insert(key_id.clone(), (0, 0.0));
    }
    walk_log_entries(log_dir, |value, key_id| {
        let entry = stats.entry(key_id.to_string()).or_insert((0, 0.0));
        entry.0 += 1;
        if let Some(cost) = value.get("cost_usd").and_then(|v| v.as_f64()) {
            entry.1 += cost;
        }
    });
    stats
        .into_iter()
        .map(|(key_id, (reqs, cost))| crate::admin_models::KeyStats {
            key_id,
            total_requests: reqs,
            total_cost_usd: cost,
        })
        .collect()
}

/// Walk all log entries across all key directories, calling f(value, key_id) for each
fn walk_log_entries(log_dir: &Path, mut f: impl FnMut(&serde_json::Value, &str)) {
    let Ok(rd) = std::fs::read_dir(log_dir) else {
        return;
    };
    for dir_entry in rd.filter_map(|e| e.ok()) {
        let dir_path = dir_entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let key_id = dir_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let Ok(files) = std::fs::read_dir(&dir_path) else {
            continue;
        };
        for file_entry in files.filter_map(|e| e.ok()) {
            let Ok(content) = std::fs::read_to_string(file_entry.path()) else {
                continue;
            };
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    f(&value, &key_id);
                }
            }
        }
    }
}
