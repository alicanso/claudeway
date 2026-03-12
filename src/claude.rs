use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use tokio::io::AsyncBufReadExt;

use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::models::{ClaudeCliOutput, SessionJSONLEntry, TokenUsage};

#[derive(Debug)]
pub struct ClaudeResult {
    pub claude_session_id: Option<String>,
    pub result: Option<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub tokens: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
}

pub async fn run_task(
    config: &Config,
    prompt: &str,
    model: Option<&str>,
    system_prompt: Option<&str>,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, AppError> {
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ];

    if let Some(m) = model {
        let resolved = resolve_model(m);
        args.push("--model".to_string());
        args.push(resolved);
    }

    if let Some(sp) = system_prompt {
        args.push("--system-prompt".to_string());
        args.push(sp.to_string());
    }

    run_claude(config, &args, workdir, timeout_secs).await
}

pub async fn run_resume(
    config: &Config,
    prompt: &str,
    claude_session_id: &str,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, AppError> {
    let args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--resume".to_string(),
        claude_session_id.to_string(),
    ];

    run_claude(config, &args, workdir, timeout_secs).await
}

async fn run_claude(
    config: &Config,
    args: &[String],
    workdir: &Path,
    timeout_secs: u64,
) -> Result<ClaudeResult, AppError> {
    let start = Instant::now();

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::process::Command::new(&config.claude_bin)
            .args(args)
            .current_dir(workdir)
            .output(),
    )
    .await
    .map_err(|_| ApiError::timeout())?
    .map_err(|e| ApiError::internal(format!("Failed to spawn claude process: {e}")))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let parsed: Option<ClaudeCliOutput> = serde_json::from_str(&stdout).ok();

    let (result, session_id, cost_usd, is_error) = if let Some(ref cli) = parsed {
        (
            cli.result.clone(),
            cli.session_id.clone(),
            cli.cost_usd,
            cli.is_error.unwrap_or(false),
        )
    } else {
        let raw = if stdout.trim().is_empty() {
            None
        } else {
            Some(stdout.clone())
        };
        (raw, None, None, false)
    };

    let success = exit_code == Some(0) && !is_error;

    let tokens = if let Some(ref sid) = session_id {
        extract_tokens_from_jsonl(sid).await
    } else {
        None
    };

    Ok(ClaudeResult {
        claude_session_id: session_id,
        result,
        success,
        exit_code,
        duration_ms,
        tokens,
        cost_usd,
    })
}

pub async fn run_task_streaming(
    config: &Config,
    prompt: &str,
    model: Option<&str>,
    system_prompt: Option<&str>,
    workdir: &Path,
    timeout_secs: u64,
    text_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<ClaudeResult, AppError> {
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ];

    if let Some(m) = model {
        args.push("--model".to_string());
        args.push(resolve_model(m));
    }

    if let Some(sp) = system_prompt {
        args.push("--system-prompt".to_string());
        args.push(sp.to_string());
    }

    run_claude_streaming(config, &args, workdir, timeout_secs, text_tx).await
}

pub async fn run_resume_streaming(
    config: &Config,
    prompt: &str,
    claude_session_id: &str,
    workdir: &Path,
    timeout_secs: u64,
    text_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<ClaudeResult, AppError> {
    let args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--resume".to_string(),
        claude_session_id.to_string(),
    ];

    run_claude_streaming(config, &args, workdir, timeout_secs, text_tx).await
}

async fn run_claude_streaming(
    config: &Config,
    args: &[String],
    workdir: &Path,
    timeout_secs: u64,
    text_tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<ClaudeResult, AppError> {
    let start = Instant::now();

    let mut child = tokio::process::Command::new(&config.claude_bin)
        .args(args)
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ApiError::internal(format!("Failed to spawn claude: {e}")))?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stdout).lines();

    let mut accumulated_text = String::new();
    let mut session_id: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut is_error = false;

    let stream_result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        async {
            while let Some(line) = reader.next_line().await? {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };

                match event.get("type").and_then(|t| t.as_str()) {
                    Some("content_block_delta") => {
                        if let Some(text) = event
                            .get("delta")
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                        {
                            accumulated_text.push_str(text);
                            let _ = text_tx.send(accumulated_text.clone());
                        }
                    }
                    Some("result") => {
                        session_id = event
                            .get("session_id")
                            .and_then(|s| s.as_str())
                            .map(String::from);
                        cost_usd = event.get("cost_usd").and_then(|c| c.as_f64());
                        is_error = event
                            .get("is_error")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);
                        if let Some(r) = event.get("result").and_then(|r| r.as_str()) {
                            accumulated_text = r.to_string();
                        }
                    }
                    _ => {}
                }
            }
            Ok::<(), std::io::Error>(())
        },
    )
    .await;

    // Signal end of stream
    drop(text_tx);

    let duration_ms = start.elapsed().as_millis() as u64;

    match stream_result {
        Err(_) => {
            let _ = child.kill().await;
            return Err(ApiError::timeout().into());
        }
        Ok(Err(e)) => {
            return Err(ApiError::internal(format!("Stream read error: {e}")).into());
        }
        Ok(Ok(())) => {}
    }

    let status = child
        .wait()
        .await
        .map_err(|e| ApiError::internal(format!("Wait error: {e}")))?;

    let exit_code = status.code();
    let success = exit_code == Some(0) && !is_error;

    let tokens = if let Some(ref sid) = session_id {
        extract_tokens_from_jsonl(sid).await
    } else {
        None
    };

    Ok(ClaudeResult {
        claude_session_id: session_id,
        result: if accumulated_text.is_empty() {
            None
        } else {
            Some(accumulated_text)
        },
        success,
        exit_code,
        duration_ms,
        tokens,
        cost_usd,
    })
}

async fn extract_tokens_from_jsonl(session_id: &str) -> Option<TokenUsage> {
    let home = dirs_next::home_dir()?;
    let projects_dir = home.join(".claude").join("projects");

    let entries = std::fs::read_dir(&projects_dir).ok()?;

    for entry in entries.flatten() {
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let jsonl_path = entry.path().join(format!("{session_id}.jsonl"));
        if jsonl_path.exists() {
            return parse_jsonl_tokens(&jsonl_path);
        }
    }

    None
}

fn parse_jsonl_tokens(path: &Path) -> Option<TokenUsage> {
    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    for line in lines.iter().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<SessionJSONLEntry>(line) {
            if let Some(message) = entry.message {
                if let Some(usage) = message.usage {
                    return Some(TokenUsage {
                        input: usage.input_tokens.unwrap_or(0),
                        output: usage.output_tokens.unwrap_or(0),
                        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
                        cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
                    });
                }
            }
        }
    }

    None
}

fn resolve_model(model: &str) -> String {
    match model {
        "sonnet" => "claude-sonnet-4-6".to_string(),
        "haiku" => "claude-haiku-4-5-20251001".to_string(),
        "opus" => "claude-opus-4-6".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_resolve_model_aliases() {
        assert_eq!(resolve_model("sonnet"), "claude-sonnet-4-6");
        assert_eq!(resolve_model("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(resolve_model("opus"), "claude-opus-4-6");
        assert_eq!(resolve_model("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(resolve_model("custom-model"), "custom-model");
    }

    #[test]
    fn test_parse_jsonl_tokens_from_string() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test-session.jsonl");

        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, r#"{{"type":"human","message":{{"usage":null}}}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":150,"output_tokens":75,"cache_creation_input_tokens":20,"cache_read_input_tokens":30}}}}}}"#
        )
        .unwrap();
        writeln!(file, r#"{{"type":"result","session_id":"abc"}}"#).unwrap();
        file.flush().unwrap();

        let tokens = parse_jsonl_tokens(&file_path).unwrap();
        assert_eq!(tokens.input, 150);
        assert_eq!(tokens.output, 75);
        assert_eq!(tokens.cache_write, 20);
        assert_eq!(tokens.cache_read, 30);
    }
}
