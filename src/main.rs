use std::net::SocketAddr;
use tokio::net::TcpListener;

mod config;
mod error;
mod models;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = axum::Router::new();
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr).await?;
    println!("Claudeway listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
