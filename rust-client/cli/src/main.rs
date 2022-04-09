#![allow(
    clippy::drop_copy,
    clippy::unused_unit,
    clippy::comparison_chain,
    clippy::let_and_return,
    clippy::redundant_pattern_matching,
    clippy::len_without_is_empty,
    clippy::useless_format,
)]

use std::io::Write;
use std::{iter, io, mem};
use std::panic::resume_unwind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use sendfile::{DownloaderClient, ProgressState, Transport, UploaderClient};
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

const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

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
            .ok_or_else(|| anyhow!("cannot disable both p2p and relay"))?;

        let send_client = UploaderClient::new(api_endpoint, download_endpoint, transport);

        let multi_progress = MultiProgress::new();

        let provision_spinner = multi_progress
            .add(ProgressBar::new_spinner().with_message("Uploading file information..."));
        provision_spinner.enable_steady_tick(PROGRESS_UPDATE_INTERVAL);

        let provisioned_file = send_client.provision_file(path)?;
        provision_spinner.finish_and_clear();

        let formatted_download_url_and_key =
            provisioned_file.formatted_download_url_and_key();

        let file_name = &path
            .file_name()
            .expect("upload file has a name")
            .to_string_lossy();
        let encrypt_spinner = multi_progress
            .add(ProgressBar::new_spinner().with_message(format!("Encrypting {file_name}...")));
        encrypt_spinner.enable_steady_tick(PROGRESS_UPDATE_INTERVAL);

        let upload_progress_bar_style = ProgressStyle::with_template(
            "{spinner:.green} {msg}\n{wide_bar:.cyan/blue} {bytes}/{total_bytes} (ETA {eta} remaining)",
        ).unwrap();
        let upload_progress_bar = multi_progress.add(
            ProgressBar::new(provisioned_file.file_size())
                .with_message(format!("Uploading {file_name}..."))
                .with_style(upload_progress_bar_style),
        );

        let finish_spinner = multi_progress
            .add(ProgressBar::new_spinner().with_message(format!("Waiting for downloader to finish...")));

        let (progress_tx, progress_rx) = mpsc::channel();
        crossbeam::scope(|scope| {
            let progress_thread = scope.spawn(|_scope| {
                run_transfer_progress(progress_rx, &upload_progress_bar, &finish_spinner)
            });

            let upload_result = send_client.upload_provisioned_file(provisioned_file, |progress| {
                if !encrypt_spinner.is_finished() {
                    encrypt_spinner.finish_and_clear();
                    eprintln!("Starting upload. Receiver can simultaneously download at:");
                    println!("{formatted_download_url_and_key}");
                    io::stdout().flush().unwrap();
                }
                progress_tx
                    .send(progress)
                    .expect("progress thread is running")
            });

            // drop our progress sender to signal the progress thread to stop, and then wait for it.
            drop(progress_tx);
            progress_thread
                .join()
                .unwrap_or_else(|panic| resume_unwind(panic));

            upload_result
        })
        .unwrap_or_else(|panic| resume_unwind(panic))?;

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
            .ok_or_else(|| anyhow!("cannot disable both p2p and relay"))?;

        let downloader_client =
            DownloaderClient::from_download_url(download_url, api_endpoint, transport)?;

        let multi_progress = MultiProgress::new();

        let meta_spinner = multi_progress
            .add(ProgressBar::new_spinner().with_message("Downloading file information..."));
        meta_spinner.enable_steady_tick(PROGRESS_UPDATE_INTERVAL);

        let meta = downloader_client.fetch_meta()?;
        meta_spinner.finish_and_clear();

        let file_name = &meta.file_meta.file_name;
        let download_progress_bar_style = ProgressStyle::with_template(
            "{spinner:.green} {msg}\n{wide_bar:.cyan/blue} {bytes}/{total_bytes} (ETA {eta} remaining)",
        )
        .unwrap();
        let download_progress_bar = multi_progress.add(
            ProgressBar::new(meta.file_meta.file_size)
                .with_message(format!("Downloading {file_name}..."))
                .with_style(download_progress_bar_style),
        );

        let decrypt_spinner = multi_progress
            .add(ProgressBar::new_spinner().with_message(format!("Decrypting {file_name}...")));

        let (progress_tx, progress_rx) = mpsc::channel();
        crossbeam::scope(|scope| {
            let progress_thread = scope.spawn(|_scope| {
                run_transfer_progress(progress_rx, &download_progress_bar, &decrypt_spinner)
            });

            let download_result =
                downloader_client.download(&meta, Some(Duration::from_secs(5)), |progress| {
                    progress_tx
                        .send(progress)
                        .expect("progress thread is running");
                });

            // drop our progress sender to signal the progress thread to stop, and then wait for it.
            drop(progress_tx);
            progress_thread
                .join()
                .unwrap_or_else(|panic| resume_unwind(panic));

            download_result
        })
        .unwrap_or_else(|panic| resume_unwind(panic))?;

        Ok(())
    }
}

fn run_transfer_progress(
    rx: mpsc::Receiver<ProgressState<u64>>,
    transfer_progress_bar: &ProgressBar,
    finish_spinner: &ProgressBar,
) {
    // Throttle progress bar position updates to prevent the displayed ETA from updating too fast.
    let mut transferred_first_byte = false;
    for progress in throttle_receiver(rx) {
        if progress.current != 0 && !mem::replace(&mut transferred_first_byte, true) {
            transfer_progress_bar.reset_eta();
        }
        transfer_progress_bar.set_position(progress.current);
        if progress.current == progress.total {
            transfer_progress_bar.finish_and_clear();
            finish_spinner.tick();
        }
    }
    transfer_progress_bar.finish_and_clear();
    finish_spinner.finish_and_clear();
}

fn throttle_receiver<T: Clone>(rx: mpsc::Receiver<T>) -> impl Iterator<Item = T> {
    let mut next_update = Instant::now();
    let mut last_progress = None;
    iter::from_fn(move || loop {
        let time_until_next_update = next_update
            .checked_duration_since(Instant::now())
            .unwrap_or_default();
        match rx.recv_timeout(time_until_next_update) {
            Ok(progress) => last_progress = Some(progress),
            Err(RecvTimeoutError::Timeout) => (),
            Err(RecvTimeoutError::Disconnected) => return None,
        }

        let now = Instant::now();
        if now >= next_update {
            next_update = now + PROGRESS_UPDATE_INTERVAL;
            if let Some(last_progress) = &last_progress {
                return Some(last_progress.clone());
            }
        }
    })
}
