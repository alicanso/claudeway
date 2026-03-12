use crate::plugin::{EventType, GatewayEvent, Plugin, PluginContext, PluginRegistrar};
use std::future::Future;
use std::pin::Pin;
use tokio::process::Command;
use tokio::sync::Mutex;

/// Cloudflare Tunnel plugin — exposes the local server via cloudflared.
///
/// Quick tunnel (zero config, random URL):
/// ```toml
/// [plugins.cloudflare_tunnel]
/// enabled = true
/// ```
///
/// Named tunnel (persistent domain, requires token from Cloudflare dashboard):
/// ```toml
/// [plugins.cloudflare_tunnel]
/// enabled = true
/// tunnel_token = "eyJhIjoiNGY..."
/// ```
pub struct CloudflareTunnelPlugin {
    tunnel_token: Option<String>,
    child: Mutex<Option<tokio::process::Child>>,
}

impl CloudflareTunnelPlugin {
    pub fn new(tunnel_token: Option<String>) -> Self {
        Self {
            tunnel_token,
            child: Mutex::new(None),
        }
    }
}

impl Plugin for CloudflareTunnelPlugin {
    fn name(&self) -> &str {
        "cloudflare_tunnel"
    }

    fn on_register(&self, registrar: &mut PluginRegistrar) -> anyhow::Result<()> {
        registrar.subscribe(EventType::ServerStarted);
        Ok(())
    }

    fn on_event(
        &self,
        event: &GatewayEvent,
        _ctx: &PluginContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        match event {
            GatewayEvent::ServerStarted { port } => {
                let port = *port;
                Box::pin(async move {
                    let child = if let Some(ref token) = self.tunnel_token {
                        // Named tunnel: uses pre-configured tunnel with custom domain
                        tracing::info!("starting cloudflared named tunnel...");
                        Command::new("cloudflared")
                            .args(["tunnel", "run", "--token", token])
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                    } else {
                        // Quick tunnel: zero config, random URL
                        tracing::info!("starting cloudflared quick tunnel on port {port}...");
                        Command::new("cloudflared")
                            .args([
                                "tunnel",
                                "--url",
                                &format!("http://localhost:{port}"),
                            ])
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                    };

                    match child {
                        Ok(mut proc) => {
                            // For quick tunnels, parse the public URL from stderr
                            if self.tunnel_token.is_none() {
                                if let Some(stderr) = proc.stderr.take() {
                                    tokio::spawn(async move {
                                        use tokio::io::{AsyncBufReadExt, BufReader};
                                        let reader = BufReader::new(stderr);
                                        let mut lines = reader.lines();
                                        while let Ok(Some(line)) = lines.next_line().await {
                                            if let Some(url) = extract_tunnel_url(&line) {
                                                tracing::info!("cloudflare tunnel URL: {url}");
                                                eprintln!(
                                                    "  \x1b[1;32m→\x1b[0m Tunnel    \x1b[1m{}\x1b[0m",
                                                    url
                                                );
                                                eprintln!();
                                            }
                                            tracing::debug!(target: "cloudflared", "{}", line);
                                        }
                                    });
                                }
                            } else if let Some(stderr) = proc.stderr.take() {
                                tokio::spawn(async move {
                                    use tokio::io::{AsyncBufReadExt, BufReader};
                                    let reader = BufReader::new(stderr);
                                    let mut lines = reader.lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        tracing::debug!(target: "cloudflared", "{}", line);
                                    }
                                });
                            }

                            {
                                let mut guard = self.child.lock().await;
                                *guard = Some(proc);
                            }
                            tracing::info!("cloudflared process started");
                        }
                        Err(e) => {
                            tracing::error!("failed to start cloudflared: {e}");
                            tracing::error!(
                                "make sure cloudflared is installed: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/"
                            );
                        }
                    }

                    Ok(())
                })
            }
            _ => Box::pin(async { Ok(()) }),
        }
    }

    fn on_shutdown(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async {
            let mut guard = self.child.lock().await;
            if let Some(ref mut child) = *guard {
                tracing::info!("shutting down cloudflared tunnel...");
                let _ = child.kill().await;
                tracing::info!("cloudflared tunnel stopped");
            }
            Ok(())
        })
    }
}

/// Extract the tunnel URL from cloudflared stderr output.
/// cloudflared prints something like: "... https://xxx-yyy-zzz.trycloudflare.com ..."
fn extract_tunnel_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|word| word.starts_with("https://") && word.contains(".trycloudflare.com"))
        .map(|s| s.to_string())
}
