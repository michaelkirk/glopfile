#![allow(
    dropping_copy_types,
    clippy::unused_unit,
    clippy::comparison_chain,
    clippy::let_and_return,
    clippy::let_unit_value,
    // UniFFI generates a large metadata array in the included scaffolding.
    clippy::large_const_arrays,
    clippy::redundant_pattern_matching,
    clippy::len_without_is_empty,
    clippy::useless_format
)]

#[macro_use]
extern crate log;

mod api_client;
mod cipher;
mod downloader_client;
mod error;
mod p2p;
mod transport;
mod uploader_client;
mod url_safe_base64;
mod util;
mod websocket;

pub use api_client::{DownloadId, DownloadMeta, FileMeta};
pub use downloader_client::DownloaderClient;
pub use error::Error;
pub use transport::Transport;
pub use uploader_client::{ProvisionedFile, UploadableFile, UploaderClient};
pub use util::ProgressState;
pub type Result<T> = std::result::Result<T, Error>;

cfg_if::cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        pub use uploader_client::{NativeUploadFile, NativeProvisionedFile};
    }
}

// The UDL scaffolding has to live at the crate root, since it refers to `crate::UniFfiTag`.
#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
use crate::Error as GlopfileError;
#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
uniffi::include_scaffolding!("lib");

#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
pub mod ffi {
    pub use crate::{
        DownloadMeta, DownloaderClient, Error as GlopfileError, FileMeta, NativeProvisionedFile,
        Transport, UniFfiTag, UploaderClient,
    };
}

use api_client::ApiClient;
use cipher::CipherKey;

#[cfg(test)]
fn init_test_logging() {
    use std::sync::Once;
    static START: Once = Once::new();
    START.call_once(|| {
        pretty_env_logger::init();
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::{fs, path::Path, time::Duration};

    const TEST_FIXTURES_DIR: &str = "test_fixtures/";
    const SAMPLE_FILE_NAME: &str = "sample_file.txt";

    /// A local transfer that takes longer than this has stalled.
    const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(10);

    struct RoundTripTest {
        uploader_transport: Transport,
        downloader_transport: Transport,
        downloader_p2p_timeout: Option<Duration>,
    }
    impl RoundTripTest {
        /// Fails the test instead of hanging forever when a transfer stalls.
        fn run(self) {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| self.transfer()));
                let _ignore = tx.send(result);
            });

            match rx.recv_timeout(ROUND_TRIP_TIMEOUT) {
                Ok(Ok(())) => (),
                Ok(Err(panic)) => resume_unwind(panic),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    panic!("transfer did not finish within {ROUND_TRIP_TIMEOUT:?}")
                }
            }
        }

        fn transfer(&self) {
            init_test_logging();

            let uploader = UploaderClient::new_testing(self.uploader_transport);
            let path = Path::new(TEST_FIXTURES_DIR).join(SAMPLE_FILE_NAME);
            let provisioned_file = uploader.provision_file(&path).unwrap();

            let download_url = provisioned_file.formatted_download_url_and_key();

            // uploading is a blocking operation, so spawn it on separate thread
            let uploader_handler = std::thread::spawn(move || {
                debug!("uploader will upload");
                uploader
                    .upload_provisioned_file(provisioned_file, drop)
                    .unwrap();
                debug!("uploader did upload");
            });

            let downloader = DownloaderClient::from_testing_download_url(
                &download_url,
                self.downloader_transport,
            )
            .unwrap();
            let output_dir = tempfile::tempdir_in(env!("OUT_DIR")).unwrap().keep();
            debug!("downloader will download");
            let meta = downloader.fetch_meta().unwrap();
            downloader
                .download(
                    &meta,
                    Some(output_dir.clone()),
                    self.downloader_p2p_timeout,
                    drop,
                )
                .unwrap();
            debug!("downloader did download");

            uploader_handler.join().unwrap();

            assert_eq!(
                fs::read(output_dir.join(SAMPLE_FILE_NAME)).unwrap(),
                fs::read(path).unwrap()
            );

            let _ignore = fs::remove_dir_all(output_dir);
        }
    }

    #[test]
    fn round_trip_relayed() {
        RoundTripTest {
            uploader_transport: Transport::Both,
            downloader_transport: Transport::Relay,
            downloader_p2p_timeout: None,
        }
        .run();
    }

    #[test]
    fn round_trip_p2p() {
        RoundTripTest {
            uploader_transport: Transport::Both,
            downloader_transport: Transport::P2P,
            downloader_p2p_timeout: Some(Duration::from_secs(15)),
        }
        .run();
    }

    /// With no peer to answer the offer, the downloader's p2p attempt times out and relay takes over.
    #[test]
    fn round_trip_fallback() {
        RoundTripTest {
            uploader_transport: Transport::Relay,
            downloader_transport: Transport::Both,
            downloader_p2p_timeout: Some(Duration::from_secs(1)),
        }
        .run();
    }
}
