use std::net::SocketAddr;

use glopfile_relay::api;
use glopfile_relay::registry::Registry;
use tokio::net::TcpListener;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let addr: SocketAddr = std::env::var("GLOPFILE_LISTEN")
        .unwrap_or_else(|_| DEFAULT_LISTEN_ADDR.to_owned())
        .parse()?;

    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "glopfile relay listening");
    axum::serve(listener, api::router(Registry::new())).await?;
    Ok(())
}
