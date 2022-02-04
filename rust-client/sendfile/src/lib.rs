#[macro_use]
extern crate log;

mod api_client;
mod cipher;
mod error;
mod receiver_client;
mod sender_client;

pub use api_client::DownloadId;
pub use error::Error;
pub use receiver_client::ReceiverClient;
pub use sender_client::SenderClient;

use api_client::ApiClient;
use cipher::CipherKey;

type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
fn init_test_logging() {
    use std::sync::Once;
    static START: Once = Once::new();
    START.call_once(|| {
        pretty_env_logger::init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn round_trip() {
        init_test_logging();

        let sender = SenderClient::new_testing();
        let path = PathBuf::from("test_fixtures/sample_file.txt");
        let provisioned_file = sender.provision_file(&path).unwrap();

        let download_url = provisioned_file.formatted_download_url_and_key();

        // uploading is a blocking operation, so spawn it on separate thread
        let sender_handler = std::thread::spawn(move || {
            debug!("sender will upload");
            sender.upload_provisioned_file(provisioned_file).unwrap();
            debug!("sender did upload");
        });

        let mut receiver = ReceiverClient::from_testing_download_url(&download_url).unwrap();
        receiver.set_output_dir(&std::env::temp_dir());
        debug!("receiver will download");
        receiver.download().unwrap();
        debug!("receiver did download");

        sender_handler.join().unwrap();
    }
}
