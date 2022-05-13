#![allow(
    clippy::drop_copy,
    clippy::unused_unit,
    clippy::comparison_chain,
    clippy::let_and_return,
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

#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
pub mod ffi {
    use crate::{
        DownloadMeta, DownloaderClient, Error as SendfileError, FileMeta, NativeProvisionedFile,
        Transport, UploaderClient,
    };

    include!(concat!(env!("OUT_DIR"), "/lib.uniffi.rs"));
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
    use std::{fs, path::Path, time::Duration};

    const TEST_FIXTURES_DIR: &str = "test_fixtures/";
    const SAMPLE_FILE_NAME: &str = "sample_file.txt";

    #[test]
    fn round_trip() {
        init_test_logging();

        let uploader = UploaderClient::new_testing();
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

        let mut downloader =
            DownloaderClient::from_testing_download_url(&download_url, Transport::Relay).unwrap();
        let output_dir = tempfile::tempdir_in(env!("OUT_DIR")).unwrap().into_path();
        downloader.set_output_dir(&output_dir);
        debug!("downloader will download");
        let meta = downloader.fetch_meta().unwrap();
        downloader.download(&meta, None, drop).unwrap();
        debug!("downloader did download");

        uploader_handler.join().unwrap();

        assert_eq!(
            fs::read(output_dir.join(SAMPLE_FILE_NAME)).unwrap(),
            fs::read(path).unwrap()
        );

        let _ignore = fs::remove_dir_all(output_dir);
    }

    #[test]
    fn round_trip_p2p() {
        init_test_logging();

        let uploader = UploaderClient::new_testing();
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

        let output_dir = tempfile::tempdir_in(env!("OUT_DIR")).unwrap().into_path();
        let mut downloader =
            DownloaderClient::from_testing_download_url(&download_url, Transport::P2P).unwrap();
        downloader.set_output_dir(&output_dir);
        debug!("downloader will download");
        let meta = downloader.fetch_meta().unwrap();
        downloader
            .download(&meta, Some(Duration::from_secs(15)), drop)
            .unwrap();
        debug!("downloader did download");

        uploader_handler.join().unwrap();

        assert_eq!(
            fs::read(output_dir.join("sample_file.txt")).unwrap(),
            fs::read(path).unwrap()
        );

        let _ignore = fs::remove_dir_all(output_dir);
    }
}
