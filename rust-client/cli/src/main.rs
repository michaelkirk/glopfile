use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use clap::{AppSettings, Parser, Subcommand};
use sendfile::{DownloaderClient, UploaderClient};
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
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Send {
        /// file you want to send
        path: PathBuf,

        /// The API service used to coordinate file transfer. If unspecified, a default will be used.
        #[clap(short, long)]
        api_endpoint: Option<Url>,

        /// The base of the generated download link. If unspecified, a default will be used.
        #[clap(short, long)]
        download_endpoint: Option<Url>,
    },

    /// Downloads a file being sent by another user
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Receive {
        /// The link provided to you by the sender
        download_link: String,

        /// The API service used to coordinate file transfer. If unspecified, a default will be used.
        #[clap(short, long)]
        api_endpoint: Option<Url>,
    },
}

fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    let args = Args::parse();

    match &args.command {
        Commands::Send { path, api_endpoint, download_endpoint } => {
            Cli::send(path, api_endpoint.as_ref(), download_endpoint.as_ref())
        }
        Commands::Receive { download_link, api_endpoint } => {
            Cli::receive(download_link, api_endpoint.as_ref())
        }
    }
}

struct Cli;
impl Cli {
    fn send(
        path: &Path,
        api_endpoint: Option<&Url>,
        download_endpoint: Option<&Url>,
    ) -> Result<()> {
        let default_api_endpoint =
            Url::parse("http://localhost:8080").expect("invalid hardcoded endpoint");
        let api_endpoint = api_endpoint.unwrap_or(&default_api_endpoint).clone();

        let default_download_endpoint =
            Url::parse("http://localhost:3000").expect("invalid hardcoded endpoint");
        let download_endpoint = download_endpoint
            .unwrap_or(&default_download_endpoint)
            .clone();

        let send_client = UploaderClient::new(api_endpoint, download_endpoint);

        let provisioned_file = send_client.provision_file(path)?;

        println!(
            "Starting upload. Receiver can simultaneously download at: {}",
            provisioned_file.formatted_download_url_and_key()
        );
        send_client.upload_provisioned_file(provisioned_file)?;

        Ok(())
    }

    fn receive(download_url: &str, api_endpoint: Option<&Url>) -> Result<()> {
        let default_api_endpoint =
            Url::parse("http://localhost:8080").expect("invalid hardcoded endpoint");
        let api_endpoint = api_endpoint.unwrap_or(&default_api_endpoint).clone();

        let downloader_client = DownloaderClient::from_download_url(download_url, api_endpoint)?;
        downloader_client.download(Some(Duration::from_secs(5)))?;
        println!("Downloaded file.");

        Ok(())
    }
}
