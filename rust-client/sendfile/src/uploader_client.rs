#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::io;
use std::ops::ControlFlow::{self, Break, Continue};

use bytes::Bytes;
use futures::future::{AbortHandle, OptionFuture};
use futures::{pin_mut, AsyncRead, Future, FutureExt};
use prost::Message;
use url::Url;

use crate::api_client::{EncryptedFile, REQUEST_TIMEOUT};
use crate::mpsc;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, DownloaderMessage,
    TransferFinished,
};
use crate::p2p::{PeerToPeerClient, PeerToPeerClientHandler, SignalingMessageHandler};
use crate::util::{abortable_timeout, Progress, ProgressState};
use crate::websocket::{web_socket_message, WebSocketClient};
use crate::{ApiClient, CipherKey, DownloadId, Error, Result, Transport};

pub struct UploaderClient {
    api_client: ApiClient,
    download_endpoint: Url,
    transport: Transport,
}

#[async_trait::async_trait(?Send)]
pub trait UploadableFile {
    async fn len(&self) -> io::Result<u64>;
}

impl UploaderClient {
    pub fn new(api_endpoint: Url, download_endpoint: Url, transport: Transport) -> Self {
        let api_client = ApiClient::new(api_endpoint, CipherKey::random());
        Self { api_client, download_endpoint, transport }
    }

    #[cfg(test)]
    pub fn new_testing() -> Self {
        let api_endpoint = option_env!("TEST_SENDFILE_API_ENDPOINT")
            .unwrap_or("http://localhost:8080")
            .parse()
            .expect("invalid TEST_SENDFILE_API_ENDPOINT");
        let download_endpoint = option_env!("TEST_SENDFILE_DOWNLOAD_ENDPOINT")
            .unwrap_or("http://localhost:3000")
            .parse()
            .expect("invalid hardcoded url");
        Self::new(api_endpoint, download_endpoint, Transport::Both)
    }

    async fn provision_file_async<F>(
        &self,
        file: F,
        file_name: String,
    ) -> Result<ProvisionedFile<F>>
    where
        F: UploadableFile + AsyncRead + 'static,
    {
        let file_size = file.len().await?;

        let provision_response = self.api_client.provision_file(file_name, file_size).await?;

        let download_id = DownloadId::new(provision_response.download_id);
        let download_url_without_cipher_key = self.download_url_without_cipher_key(&download_id);

        // If the server ever changes to return non-relative URL's we'll have to change this logic
        // to try both relative and absolute URLs
        let upload_path = provision_response.upload_url;
        let provisioned_file = ProvisionedFile {
            file,
            file_size,
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

    pub fn upload_provisioned_file_async<F: AsyncRead + 'static>(
        &self,
        provisioned_file: ProvisionedFile<F>,
    ) -> Progress<u64, impl Future<Output = Result<()>> + '_> {
        Progress::new_with(|progress_tx| async move {
            let file = provisioned_file.file;
            pin_mut!(file);
            let encrypted_file = self
                .api_client
                .encrypt_file(file, provisioned_file.file_size)
                .await?;
            let encrypted_file_len = encrypted_file.len();

            // Send a progress update to signal that we're done with encryption and about to start the upload.
            let _ignore = progress_tx.send(ProgressState { current: 0, total: encrypted_file_len });

            // Set up the the relay task if necessary.
            let upload_path = &provisioned_file.upload_path;
            let (relay_request_timeout_handle, relay_task) = match self.transport {
                Transport::Relay | Transport::Both => {
                    let relay_task = self
                        .api_client
                        .upload_file(encrypted_file.clone(), upload_path);
                    let (relay_task, relay_request_timeout_handle) =
                        abortable_timeout(REQUEST_TIMEOUT, relay_task);
                    (Some(relay_request_timeout_handle), Some(relay_task.fuse()))
                }
                Transport::P2P => (None, None),
            };
            let relay_task = OptionFuture::from(relay_task);

            // Set up the the P2P client if necessary.
            let (signaling_message_handler, p2p_client) = match self.transport {
                Transport::P2P | Transport::Both => {
                    let p2p_client = PeerToPeerClient::new()?;
                    (
                        Some(p2p_client.signaling_message_handler()),
                        Some(p2p_client),
                    )
                }
                Transport::Relay => (None, None),
            };

            // Connect to the websocket.
            let websocket = self
                .connect_websocket(
                    upload_path,
                    encrypted_file_len,
                    progress_tx.clone(),
                    relay_request_timeout_handle,
                    signaling_message_handler,
                )
                .await?;

            // Set up the P2P task if necessary.
            let state = UploadState { encrypted_file, progress_tx };
            let p2p_task = p2p_client.map(|mut p2p_client| {
                let mut state = state.clone();
                let websocket = websocket.clone();
                async move {
                    p2p_client.set_websocket(websocket).await?;
                    p2p_client.transfer(&mut state, None).await?;
                    Ok::<_, Error>(())
                }
            });
            let p2p_task = OptionFuture::from(p2p_task.map(FutureExt::fuse));

            // Drive both the P2P and relay tasks.
            pin_mut!(p2p_task, relay_task);
            loop {
                futures::select! {
                    p2p_result = p2p_task => if let Some(Err(error)) = p2p_result {
                        warn!("error uploading via p2p; continuing relayed: {error}");
                    },
                    relay_result = relay_task => if let Some(relay_result) = relay_result {
                        let () = relay_result??;
                        break;
                    },
                }
            }

            // Close the websocket.
            drop(websocket);
            Ok(())
        })
    }

    async fn connect_websocket(
        &self,
        upload_path: &str,
        encrypted_file_len: u64,
        progress_tx: mpsc::Sender<ProgressState<u64>>,
        mut relay_request_timeout_handle: Option<AbortHandle>,
        signaling_message_handler: Option<SignalingMessageHandler>,
    ) -> Result<WebSocketClient> {
        let api_client = &self.api_client;
        let future = api_client.connect_upload_websocket(upload_path, move |message| {
            match message.inner {
                Some(web_socket_message::Inner::RtcSignaling(message)) => {
                    if let Some(signaling_message_handler) = &signaling_message_handler {
                        signaling_message_handler
                            .handle(message)
                            .map(Continue)
                            .unwrap_or(Break(()))
                    } else {
                        debug!("ignoring received RTC signaling message: {message:?}");
                        Continue(())
                    }
                }
                Some(web_socket_message::Inner::UploadDataAck(ack)) => {
                    let _ignore = progress_tx
                        .send(ProgressState { current: ack.offset, total: encrypted_file_len });
                    if let Some(relay_request_timeout_handle) = relay_request_timeout_handle.take()
                    {
                        relay_request_timeout_handle.abort();
                    }
                    Continue(())
                }
                None => {
                    // Unfortunately, with prost there's no way to log about what message type this actually was.
                    warn!("unhandled websocket message type");
                    Continue(())
                }
            }
        });
        future.await
    }
}

// Being generic over the file type works better than dynamic dispatch here, since with dynamic dispatch we have to
// choose whether the file is Send and thus whether ProvisionedFile is Send. However, our native implementation is Send
// while our WASM implementation isn't.
pub struct ProvisionedFile<F> {
    file: F,
    file_size: u64,
    upload_path: String,
    download_url_without_cipher_key: Url,
    cipher_key: CipherKey,
}

impl<F> ProvisionedFile<F> {
    /// The download link to get the uploaded file.
    pub fn formatted_download_url_and_key(&self) -> String {
        format!(
            "{}#cipher_key={}",
            self.download_url_without_cipher_key,
            self.cipher_key.serialized()
        )
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

#[derive(Clone)]
struct UploadState {
    encrypted_file: EncryptedFile,
    progress_tx: mpsc::Sender<ProgressState<u64>>,
}

#[async_trait::async_trait(?Send)]
impl PeerToPeerClientHandler for UploadState {
    async fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>> {
        let Self { encrypted_file, progress_tx, .. } = self;
        let message = DownloaderMessage::decode(message_data).map_err(Error::rtc_err)?;
        match message.inner {
            Some(downloader_message::Inner::DataRequest(DataRequest { offset, len })) => {
                debug!("received RTC DataRequest from downloader for offset {offset} len {len}");
                let new_offset = offset + len;
                let len = usize::try_from(len).expect("file fits in memory");
                let data = encrypted_file.read_at_exact(offset, len);
                client.send_uploader_message(uploader_message::Inner::DataResponse(
                    DataResponse { data, offset },
                ))?;
                let _ = progress_tx
                    .send(ProgressState { current: new_offset, total: encrypted_file.len() });
                Ok(Continue(()))
            }
            Some(downloader_message::Inner::TransferFinished(TransferFinished {})) => {
                debug!("outgoing RTC transfer finished");
                let _ = progress_tx.send(ProgressState {
                    current: encrypted_file.len(),
                    total: encrypted_file.len(),
                });
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

    pub(super) struct NullUploadableFile;

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

        #[test]
        fn test_formatted_download_link() {
            init_test_logging();

            let key = CipherKey::from_bytes([1u8; 32]);
            let p = ProvisionedFile {
                file: Box::pin(NullUploadableFile),
                file_size: 0,
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
    }

    #[async_trait::async_trait(?Send)]
    impl UploadableFile for NullUploadableFile {
        async fn len(&self) -> std::io::Result<u64> {
            unimplemented!()
        }
    }

    impl AsyncRead for NullUploadableFile {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            unimplemented!()
        }
    }
}
