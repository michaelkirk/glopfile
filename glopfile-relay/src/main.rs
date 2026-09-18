use std::net::{IpAddr, SocketAddr};

use clap::Parser;
use glopfile_relay::api;
use glopfile_relay::registry::Registry;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "glopfile-relay", about = "Relay for glopfile transfers")]
struct Args {
    /// Address to bind. Use 127.0.0.1 to accept only local connections.
    #[arg(short, long, default_value = "0.0.0.0")]
    address: IpAddr,

    /// Port to listen on.
    #[arg(short, long, default_value_t = 8080)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let listener = TcpListener::bind(SocketAddr::new(args.address, args.port)).await?;
    tracing::info!(addr = %listener.local_addr()?, "glopfile relay listening");
    axum::serve(listener, api::router(Registry::new())).await?;
    Ok(())
}
