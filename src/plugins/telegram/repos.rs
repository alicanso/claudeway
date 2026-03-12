use std::path::Path;

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub full_name: String,  // "owner/repo"
    pub clone_url: String,  // "https://github.com/owner/repo.git"
}

/// Run `gh repo list` for the authenticated user and all accessible orgs.
/// Returns a list of RepoInfo sorted by full_name.
pub async fn discover_repos() -> anyhow::Result<Vec<RepoInfo>> {
    // Step 1: Get authenticated user login
    let user_output = tokio::process::Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .env_remove("CLAUDECODE")
        .output()
        .await?;

    if !user_output.status.success() {
        anyhow::bail!(
            "gh api user failed: {}",
            String::from_utf8_lossy(&user_output.stderr)
        );
    }

    let username = String::from_utf8_lossy(&user_output.stdout)
        .trim()
        .to_string();

    // Step 2: Get org list
    let org_output = tokio::process::Command::new("gh")
        .args(["org", "list"])
        .env_remove("CLAUDECODE")
        .output()
        .await?;

    let mut owners = vec![username];
    if org_output.status.success() {
        let org_str = String::from_utf8_lossy(&org_output.stdout);
        for line in org_str.lines() {
            let org = line.split_whitespace().next().unwrap_or("").trim();
            if !org.is_empty() {
                owners.push(org.to_string());
            }
        }
    }

    // Step 3: Run gh repo list for each owner
    let mut repos = Vec::new();
    for owner in &owners {
        let output = tokio::process::Command::new("gh")
            .args([
                "repo",
                "list",
                owner,
                "--limit",
                "1000",
                "--json",
                "nameWithOwner,url",
                "-q",
                ".[] | \"\\(.nameWithOwner)\\t\\(.url)\"",
            ])
            .env_remove("CLAUDECODE")
            .output()
            .await?;

        if !output.status.success() {
            continue; // skip orgs we can't list
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                repos.push(RepoInfo {
                    full_name: parts[0].to_string(),
                    clone_url: format!("{}.git", parts[1]),
                });
            }
        }
    }

    repos.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(repos)
}

/// Format a page of repos as a numbered list for Telegram.
/// Returns (message_text, has_more_pages).
pub fn format_repo_page(repos: &[RepoInfo], page: usize, per_page: usize) -> (String, bool) {
    let start = page * per_page;
    if start >= repos.len() {
        return ("No more repositories.".to_string(), false);
    }
    let end = (start + per_page).min(repos.len());
    let has_more = end < repos.len();

    let mut msg = String::from("Select a repository:\n\n");
    for (i, repo) in repos[start..end].iter().enumerate() {
        msg.push_str(&format!("{}. {}\n", start + i + 1, repo.full_name));
    }

    if has_more {
        msg.push_str(&format!(
            "\nShowing {}-{} of {}. Send /next for more.",
            start + 1,
            end,
            repos.len()
        ));
    } else if page > 0 {
        msg.push_str(&format!(
            "\nShowing {}-{} of {}.",
            start + 1,
            end,
            repos.len()
        ));
    }

    (msg, has_more)
}

/// Ensure a repo is available locally. Clone if missing, pull if exists.
pub async fn ensure_repo(
    repo: &RepoInfo,
    projects_dir: &Path,
) -> anyhow::Result<std::path::PathBuf> {
    let repo_path = projects_dir.join(&repo.full_name);

    if repo_path.exists() {
        // git pull
        let pull = tokio::process::Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(&repo_path)
            .env_remove("CLAUDECODE")
            .output()
            .await?;

        if !pull.status.success() {
            tracing::warn!(
                repo = %repo.full_name,
                stderr = %String::from_utf8_lossy(&pull.stderr),
                "git pull warning"
            );
            // proceed anyway — might have local changes
        }
    } else {
        // git clone
        if let Some(parent) = repo_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let clone = tokio::process::Command::new("git")
            .args(["clone", &repo.clone_url, &repo_path.to_string_lossy()])
            .env_remove("CLAUDECODE")
            .output()
            .await?;

        if !clone.status.success() {
            anyhow::bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&clone.stderr)
            );
        }
    }

    Ok(repo_path)
}
