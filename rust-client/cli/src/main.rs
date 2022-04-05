use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use sendfile::{DownloaderClient, Transport, UploaderClient};
use url::Url;

#[derive(Parser)]
#[clap(name = "sendfile")]
#[clap(about = "send a file!", long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Make a file available via a download link until your peer downloads it.
    #[clap(arg_required_else_help(true))]
    Send {
        /// file you want to send
        path: PathBuf,

        /// The API service used to coordinate file transfer. If unspecified, a default will be used.
        #[clap(short, long)]
        api_endpoint: Option<Url>,

        /// The base of the generated download link. If unspecified, a default will be used.
        #[clap(short, long)]
        download_endpoint: Option<Url>,

        /// Do not try to send content to the downloader via a peer connection; only use the relay.
        #[clap(long)]
        no_p2p: bool,

        /// Do not try to send content via the relay; only use the peer connection. Note that
        /// encrypted metadata will still be sent via the service.
        #[clap(long)]
        no_relay: bool,
    },

    /// Downloads a file being sent by another user
    #[clap(arg_required_else_help(true))]
    Receive {
        /// The link provided to you by the sender
        download_link: String,

        /// The API service used to coordinate file transfer. If unspecified, a default will be used.
        #[clap(short, long)]
        api_endpoint: Option<Url>,

        /// Do not try to receive content from the sender via a peer connection; only use the relay.
        #[clap(long)]
        no_p2p: bool,

        /// Do not try to receive content via the relay; only use the peer connection. Note that
        /// encrypted metadata will still be received via the service.
        #[clap(long)]
        no_relay: bool,
    },
}

fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    let args = Args::parse();

    match &args.command {
        Commands::Send { path, api_endpoint, download_endpoint, no_p2p, no_relay } => Cli::send(
            path,
            api_endpoint.as_ref(),
            download_endpoint.as_ref(),
            *no_p2p,
            *no_relay,
        ),
        Commands::Receive { download_link, api_endpoint, no_p2p, no_relay } => {
            Cli::receive(download_link, api_endpoint.as_ref(), *no_p2p, *no_relay)
        }
    }
}

struct Cli;
impl Cli {
    fn send(
        path: &Path,
        api_endpoint: Option<&Url>,
        download_endpoint: Option<&Url>,
        no_p2p: bool,
        no_relay: bool,
    ) -> Result<()> {
        let default_api_endpoint =
            Url::parse("http://localhost:8080").expect("invalid hardcoded endpoint");
        let api_endpoint = api_endpoint.unwrap_or(&default_api_endpoint).clone();

        let default_download_endpoint =
            Url::parse("http://localhost:3000").expect("invalid hardcoded endpoint");
        let download_endpoint = download_endpoint
            .unwrap_or(&default_download_endpoint)
            .clone();

        let transport = Transport::with_p2p_and_relay(!no_p2p, !no_relay)
            .ok_or(anyhow!("cannot disable both p2p and relay"))?;

        let send_client = UploaderClient::new(api_endpoint, download_endpoint, transport);

        let provisioned_file = send_client.provision_file(path)?;

        println!(
            "Starting upload. Receiver can simultaneously download at: {}",
            provisioned_file.formatted_download_url_and_key()
        );
        send_client.upload_provisioned_file(provisioned_file)?;

        Ok(())
    }

    fn receive(
        download_url: &str,
        api_endpoint: Option<&Url>,
        no_p2p: bool,
        no_relay: bool,
    ) -> Result<()> {
        let default_api_endpoint =
            Url::parse("http://localhost:8080").expect("invalid hardcoded endpoint");
        let api_endpoint = api_endpoint.unwrap_or(&default_api_endpoint).clone();

        let transport = Transport::with_p2p_and_relay(!no_p2p, !no_relay)
            .ok_or(anyhow!("cannot disable both p2p and relay"))?;

        let downloader_client =
            DownloaderClient::from_download_url(download_url, api_endpoint, transport)?;
        downloader_client.download(Some(Duration::from_secs(5)))?;
        println!("Downloaded file.");

        Ok(())
    }
}
