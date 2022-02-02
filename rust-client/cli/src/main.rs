use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{AppSettings, Parser, Subcommand};
use sendfile::{ReceiverClient, SenderClient};
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

        /// non-default endpoint to coordinate file transfer
        #[clap(short, long)]
        endpoint: Option<Url>,
    },

    /// Downloads a file being sent by another user
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Receive {
        /// The link provided to you by the sender
        download_link: String,
    },
}

fn main() -> Result<()> {
    pretty_env_logger::init_timed();
    let args = Args::parse();

    match &args.command {
        Commands::Send { path, endpoint } => Cli::send(path, endpoint.as_ref()),
        Commands::Receive { download_link } => Cli::receive(download_link),
    }
}

struct Cli;
impl Cli {
    fn send(path: &Path, endpoint: Option<&Url>) -> Result<()> {
        let default_endpoint =
            Url::parse("http://localhost:8080").expect("invalid hardcoded endpoint");
        let endpoint = endpoint.unwrap_or(&default_endpoint).clone();

        let send_client = SenderClient::new(endpoint);

        let provisioned_file = send_client.provision_file(path)?;

        println!(
            "Starting upload. Receiver can simultaneously download at: {}",
            provisioned_file.formatted_download_url_and_key()
        );
        send_client.upload_provisioned_file(provisioned_file)?;

        Ok(())
    }

    fn receive(download_url: &str) -> Result<()> {
        let receiver_client = ReceiverClient::from_download_url(download_url)?;
        receiver_client.download()?;
        println!("Downloaded file.");

        Ok(())
    }
}
