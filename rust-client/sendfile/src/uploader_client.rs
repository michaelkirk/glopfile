use std::fs::File;
use std::ops::ControlFlow::{self, Break, Continue};
use std::path::Path;
use std::thread;

use bytes::Bytes;
use prost::Message;
use url::Url;

use crate::api_client::EncryptedFile;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, DownloaderMessage,
    TransferFinished,
};
use crate::p2p::{PeerToPeerClient, PeerToPeerClientHandler};
use crate::websocket::web_socket_message;
use crate::{ApiClient, CipherKey, DownloadId, Error, Result};

pub struct UploaderClient {
    api_client: ApiClient,
    download_endpoint: Url,
}

impl UploaderClient {
    pub fn new(api_endpoint: Url, download_endpoint: Url) -> Self {
        let api_client = ApiClient::new(api_endpoint, CipherKey::random());
        Self { api_client, download_endpoint }
    }

    pub fn new_testing() -> Self {
        let api_endpoint = Url::parse("http://localhost:8080").expect("invalid hardcoded url");
        let download_endpoint = Url::parse("http://localhost:3000").expect("invalid hardcoded url");
        Self::new(api_endpoint, download_endpoint)
    }

    pub fn provision_file(&self, path: &Path) -> Result<ProvisionedFile> {
        let file = File::open(path)?;

        let file_size = file.metadata()?.len();
        let file_name = path
            .file_name()
            .ok_or(Error::InvalidInput("invalid file path"))?;

        let provision_response = self
            .api_client
            .provision_file(file_name.to_string_lossy().to_string(), file_size)?;

        let download_id = DownloadId::new(provision_response.download_id);
        let download_url_without_cipher_key = self.download_url_without_cipher_key(&download_id);

        // If the server ever changes to return non-relative URL's we'll have to change this logic
        // to try both relative and absolute URLs
        let upload_path = provision_response.upload_url;
        let provisioned_file = ProvisionedFile {
            file,
            upload_path,
            download_url_without_cipher_key,
            // TODO: per file key?
            cipher_key: self.api_client.cipher_key().clone(),
        };

        Ok(provisioned_file)
    }

    fn download_url_without_cipher_key(&self, download_id: &DownloadId) -> Url {
        let mut url = self.download_endpoint.clone();
        url.set_path(&format!("/download/{}", download_id));
        url
    }

    pub fn upload_provisioned_file(&self, provisioned_file: ProvisionedFile) -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let encrypted_file = self.api_client.encrypt_file(provisioned_file.file);
        let mut p2p_client = PeerToPeerClient::new()?;
        let signaling_message_handler = p2p_client.signaling_message_handler();
        let websocket = runtime.block_on(async {
            self.api_client
                .connect_upload_websocket(&provisioned_file.upload_path, move |message| {
                    match message.inner {
                        Some(web_socket_message::Inner::RtcSignaling(message)) => {
                            signaling_message_handler
                                .handle(message)
                                .map(Continue)
                                .unwrap_or(Break(()))
                        }
                        None => {
                            // Unfortunately, with prost there's no way to log about what message type this actually was.
                            warn!("unhandled websocket message type");
                            Continue(())
                        }
                    }
                })
                .await
        })?;
        let mut state = UploadState { encrypted_file: encrypted_file.clone() };
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime
                .block_on(async {
                    p2p_client.set_websocket(websocket).await?;
                    p2p_client.transfer(&mut state, None).await
                })
                .unwrap();
        });
        self.api_client
            .upload_file(encrypted_file, &provisioned_file.upload_path)
    }
}

#[derive(Debug)]
pub struct ProvisionedFile {
    file: File,
    upload_path: String,
    download_url_without_cipher_key: Url,
    cipher_key: CipherKey,
}

impl ProvisionedFile {
    /// The download link to get the uploaded file.
    pub fn formatted_download_url_and_key(&self) -> String {
        format!(
            "{}#cipher_key={}",
            self.download_url_without_cipher_key,
            self.cipher_key.serialized()
        )
    }
}

struct UploadState {
    encrypted_file: EncryptedFile,
}

impl PeerToPeerClientHandler for UploadState {
    fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>> {
        let Self { encrypted_file } = self;
        let message = DownloaderMessage::decode(message_data).map_err(Error::rtc_err)?;
        match message.inner {
            Some(downloader_message::Inner::DataRequest(DataRequest { offset, len })) => {
                debug!("received RTC DataRequest from downloader for offset {offset} len {len}");
                let len = usize::try_from(len).expect("file fits in memory");
                let data = encrypted_file.read_at_exact(offset, len);
                client.send_uploader_message(uploader_message::Inner::DataResponse(
                    DataResponse { data, offset },
                ))?;
                Ok(Continue(()))
            }
            Some(downloader_message::Inner::TransferFinished(TransferFinished {})) => {
                debug!("outgoing RTC transfer finished");
                Ok(Break(()))
            }
            None => {
                // Unfortunately, with prost there's no way to log about what message type this actually was.
                warn!("unhandled RTC data channel message type from downloader");
                Ok(Continue(()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::init_test_logging;
    use crate::uploader_client::*;

    #[test]
    fn test_download_url() {
        init_test_logging();

        let uploader_client = UploaderClient::new_testing();
        let download_id = DownloadId::new("abc123".to_string());
        let url = uploader_client.download_url_without_cipher_key(&download_id);
        assert_eq!(
            url,
            Url::parse("http://localhost:3000/download/abc123").unwrap()
        );
    }

    mod provisioned_file_tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn test_formatted_download_link() {
            init_test_logging();

            let key = CipherKey::from_bytes([1u8; 32]);
            let p = ProvisionedFile {
                file: File::open("test_fixtures/sample_file.txt").unwrap(),
                upload_path: "/path/to/upload/123".to_string(),
                download_url_without_cipher_key: Url::parse(
                    "https://123.invalid:1234/their/download/456",
                )
                .unwrap(),
                cipher_key: key,
            };
            assert_eq!(
                p.formatted_download_url_and_key(),
                "https://123.invalid:1234/their/download/456#cipher_key=AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE~"
            )
        }

        #[test]
        fn upload_non_existent_file() {
            init_test_logging();

            let uploader = UploaderClient::new_testing();
            let path = PathBuf::from("path/to/non-existent-file");
            assert!(matches!(
                uploader.provision_file(&path),
                Err(Error::IO { .. }),
            ));
        }
    }
}
