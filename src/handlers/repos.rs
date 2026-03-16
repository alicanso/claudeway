use axum::extract::Extension;
use axum::Json;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::error::{ApiError, AppError};
use crate::models::{RepoInfo, RepoListResponse, RepoSyncRequest, RepoSyncResponse};

/// Extract repo name from a git URL
fn extract_repo_name(url: &str) -> Result<String, AppError> {
    let url = url.trim_end_matches('/');
    let last = url.rsplit('/').next().unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        return Err(ApiError::bad_request("Cannot extract repo name from URL"));
    }
    Ok(name.to_string())
}

/// Run a git command and return stdout
async fn git_command(args: &[&str], cwd: Option<&PathBuf>) -> Result<String, AppError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ApiError::internal("git binary not found")
        } else {
            ApiError::internal(format!("Failed to execute git: {e}"))
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::internal(format!("git error: {stderr}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[utoipa::path(
    post,
    path = "/repos/sync",
    tag = "Repos",
    summary = "Sync a git repository",
    description = "Clone or update a git repository in the configured repos directory.",
    request_body = RepoSyncRequest,
    responses(
        (status = 200, description = "Repository synced successfully", body = RepoSyncResponse),
        (status = 400, description = "Invalid request", body = crate::error::ApiError),
        (status = 500, description = "Git operation failed", body = crate::error::ApiError)
    ),
    security(("bearer" = []))
)]
pub async fn sync_repo(
    Extension(config): Extension<Arc<Config>>,
    Json(req): Json<RepoSyncRequest>,
) -> Result<Json<RepoSyncResponse>, AppError> {
    if req.url.trim().is_empty() {
        return Err(ApiError::bad_request("url is required"));
    }

    let repo_name = extract_repo_name(&req.url)?;
    let repos_dir = PathBuf::from(&config.repos_dir);
    let repo_path = repos_dir.join(&repo_name);

    // Ensure repos_dir exists
    tokio::fs::create_dir_all(&repos_dir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create repos directory: {e}")))?;

    let status;
    if repo_path.exists() {
        // Update existing repo
        git_command(&["fetch"], Some(&repo_path)).await?;
        if let Some(ref branch) = req.branch {
            git_command(&["checkout", branch], Some(&repo_path)).await?;
        }
        git_command(&["pull"], Some(&repo_path)).await?;
        status = "updated";
    } else {
        // Clone new repo
        git_command(
            &["clone", &req.url, &repo_path.to_string_lossy()],
            None,
        )
        .await?;
        if let Some(ref branch) = req.branch {
            git_command(&["checkout", branch], Some(&repo_path)).await?;
        }
        status = "cloned";
    }

    // Get current branch and commit
    let branch = git_command(&["branch", "--show-current"], Some(&repo_path)).await?;
    let commit = git_command(&["rev-parse", "HEAD"], Some(&repo_path)).await?;

    Ok(Json(RepoSyncResponse {
        path: repo_path.to_string_lossy().to_string(),
        status: status.to_string(),
        branch,
        commit,
    }))
}

#[utoipa::path(
    get,
    path = "/repos",
    tag = "Repos",
    summary = "List synced repositories",
    description = "List all git repositories in the configured repos directory.",
    responses(
        (status = 200, description = "Repository list", body = RepoListResponse)
    ),
    security(("bearer" = []))
)]
pub async fn list_repos(
    Extension(config): Extension<Arc<Config>>,
) -> Result<Json<RepoListResponse>, AppError> {
    let repos_dir = PathBuf::from(&config.repos_dir);
    let mut repos = Vec::new();

    if repos_dir.exists() {
        let mut entries = tokio::fs::read_dir(&repos_dir)
            .await
            .map_err(|e| ApiError::internal(format!("Failed to read repos directory: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| ApiError::internal(format!("Failed to read directory entry: {e}")))?
        {
            let path = entry.path();
            if !path.is_dir() || !path.join(".git").exists() {
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let remote_url = git_command(&["remote", "get-url", "origin"], Some(&path))
                .await
                .unwrap_or_default();
            let branch = git_command(&["branch", "--show-current"], Some(&path))
                .await
                .unwrap_or_default();
            let commit = git_command(&["rev-parse", "HEAD"], Some(&path))
                .await
                .unwrap_or_default();

            repos.push(RepoInfo {
                name,
                path: path.to_string_lossy().to_string(),
                branch,
                commit,
                remote_url,
            });
        }
    }

    Ok(Json(RepoListResponse {
        repos_dir: config.repos_dir.clone(),
        repos,
    }))
}
