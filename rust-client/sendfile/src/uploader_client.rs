#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
mod ffi;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::io;
use std::ops::ControlFlow::{self, Break, Continue};
use std::sync::Arc;

use backoff::ExponentialBackoff;
use bytes::Bytes;
use crossbeam_utils::atomic::AtomicCell;
use futures::channel::mpsc;
use futures::future::{AbortHandle, OptionFuture};
use futures::{pin_mut, AsyncRead, Future, FutureExt};
use prost::Message;
use url::Url;

use crate::api_client::{EncryptedFile, REQUEST_TIMEOUT};
use crate::error::AsRetriableResultExt;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, DownloaderHello,
    DownloaderMessage, TransferFinished, UploaderHello,
};
use crate::p2p::{PeerToPeerClient, PeerToPeerClientHandler, SignalingMessageHandler};
use crate::util::{retry, Progress, ProgressState, TimeoutExt, TimeoutResult};
use crate::websocket::{web_socket_message, WebSocketMessage, WebSocketMessageHandler};
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

#[async_trait::async_trait(?Send)]
pub trait TryClone: Sized {
    async fn try_clone(&self) -> Result<Self>;
}

impl UploaderClient {
    pub fn new(api_endpoint: Url, download_endpoint: Url, transport: Transport) -> Self {
        let api_client = ApiClient::new(api_endpoint, CipherKey::random());
        Self { api_client, download_endpoint, transport }
    }

    #[cfg(test)]
    pub fn new_testing(transport: Transport) -> Self {
        let api_endpoint = option_env!("TEST_SENDFILE_API_ENDPOINT")
            .unwrap_or("http://localhost:8080")
            .parse()
            .expect("invalid TEST_SENDFILE_API_ENDPOINT");
        let download_endpoint = option_env!("TEST_SENDFILE_DOWNLOAD_ENDPOINT")
            .unwrap_or("http://localhost:3000")
            .parse()
            .expect("invalid hardcoded url");
        Self::new(api_endpoint, download_endpoint, transport)
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
            let _ignore =
                progress_tx.unbounded_send(ProgressState { current: 0, total: encrypted_file_len });

            // Set up the the relay task if necessary.
            let upload_path = &provisioned_file.upload_path;
            let relay_request_timeout_handle = RelayRequestTimeoutHandle::default();
            let relay_task = match self.transport {
                Transport::Relay | Transport::Both => {
                    let encrypted_file = encrypted_file.clone();
                    let relay_request_timeout_handle = relay_request_timeout_handle.clone();
                    let backoff = ExponentialBackoff::default();
                    let relay_task = retry(backoff, move || {
                        let encrypted_file = encrypted_file.clone();
                        let relay_request_timeout_handle = relay_request_timeout_handle.clone();
                        async move {
                            let relay_task = self
                                .api_client
                                .upload_file(encrypted_file, upload_path)
                                .abortable_timeout(REQUEST_TIMEOUT);
                            relay_request_timeout_handle.put(relay_task.timeout_handle().clone());
                            let result: TimeoutResult<_> = relay_task.await;
                            let result: Result<_> = result.as_retriable_result()?;
                            let () = result.as_retriable_result()?;
                            Ok(())
                        }
                    });
                    Some(relay_task.fuse())
                }
                Transport::P2P => None,
            };
            let relay_task = OptionFuture::from(relay_task);

            let mut websocket_handler = UploadWebSocketHandler {
                encrypted_file_len,
                progress_tx: progress_tx.clone(),
                relay_request_timeout_handle: relay_request_timeout_handle.clone(),
                signaling_message_handler: None,
            };

            // Set up the the P2P client if necessary.
            let websocket_client;
            let p2p_task: OptionFuture<_> = match self.transport {
                Transport::P2P | Transport::Both => {
                    let mut state =
                        UploadState { encrypted_file, progress_tx: progress_tx.clone() };
                    let mut p2p_client = PeerToPeerClient::new()?;
                    websocket_handler.signaling_message_handler =
                        Some(p2p_client.signaling_message_handler());
                    websocket_client = self
                        .api_client
                        .connect_upload_websocket(upload_path, websocket_handler)?;
                    p2p_client.set_websocket_client(websocket_client.clone());
                    Some(async move { p2p_client.transfer(&mut state, None).await }.fuse()).into()
                }
                Transport::Relay => {
                    // We need to stay connected to websocket even for relay-only transfer, for upload progress updates.
                    websocket_client = self
                        .api_client
                        .connect_upload_websocket(upload_path, websocket_handler)?;
                    None.into()
                }
            };

            // Drive the P2P & relay tasks.
            pin_mut!(p2p_task, relay_task);
            loop {
                futures::select! {
                    p2p_result = p2p_task => match p2p_result {
                        Some(Ok(())) => match self.transport {
                            Transport::P2P => break Ok(()),
                            Transport::Both => {
                                // Fall through and wait for relayed upload to finish as well.
                            }
                            Transport::Relay => unreachable!(),
                        },
                        Some(Err(error)) => match self.transport {
                            Transport::P2P => break Err(error),
                            Transport::Both => warn!("error uploading via p2p; continuing relayed: {error}"),
                            Transport::Relay => unreachable!(),
                        },
                        None => (),
                    },
                    relay_result = relay_task => if let Some(relay_result) = relay_result {
                        drop(websocket_client);
                        break relay_result;
                    },
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct UploadWebSocketHandler {
    encrypted_file_len: u64,
    progress_tx: mpsc::UnboundedSender<ProgressState<u64>>,
    relay_request_timeout_handle: RelayRequestTimeoutHandle,
    signaling_message_handler: Option<SignalingMessageHandler>,
}

impl WebSocketMessageHandler for UploadWebSocketHandler {
    fn handle(&mut self, message: WebSocketMessage) -> ControlFlow<()> {
        match message.inner {
            Some(web_socket_message::Inner::RtcSignaling(message)) => {
                if let Some(signaling_message_handler) = &self.signaling_message_handler {
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
                let _ignore = self.progress_tx.unbounded_send(ProgressState {
                    current: ack.offset,
                    total: self.encrypted_file_len,
                });
                self.relay_request_timeout_handle.cancel();
                Continue(())
            }
            None => {
                // Unfortunately, with prost there's no way to log about what message type this actually was.
                warn!("unhandled websocket message type");
                Continue(())
            }
        }
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

#[async_trait::async_trait(?Send)]
impl<F: TryClone> TryClone for ProvisionedFile<F> {
    async fn try_clone(&self) -> Result<Self> {
        let file = self.file.try_clone().await?;
        Ok(Self {
            file,
            file_size: self.file_size,
            upload_path: self.upload_path.clone(),
            download_url_without_cipher_key: self.download_url_without_cipher_key.clone(),
            cipher_key: self.cipher_key.clone(),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeUploadFile;

#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeProvisionedFile;

#[derive(Clone)]
struct UploadState {
    encrypted_file: EncryptedFile,
    progress_tx: mpsc::UnboundedSender<ProgressState<u64>>,
}

#[async_trait::async_trait(?Send)]
impl PeerToPeerClientHandler for UploadState {
    type Output = ();

    async fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>> {
        let Self { encrypted_file, progress_tx, .. } = self;
        let message = DownloaderMessage::decode(message_data).map_err(Error::rtc_err)?;
        match message.inner {
            Some(downloader_message::Inner::Hello(DownloaderHello {})) => {
                client.send_uploader_message(uploader_message::Inner::Hello(UploaderHello {}))?;
                Ok(Continue(()))
            }
            Some(downloader_message::Inner::DataRequest(DataRequest { offset, len })) => {
                debug!("received RTC DataRequest from downloader for offset {offset} len {len}");
                let new_offset = offset + len;
                let len = usize::try_from(len).expect("file fits in memory");
                let data = encrypted_file.read_at_exact(offset, len);
                client.send_uploader_message(uploader_message::Inner::DataResponse(
                    DataResponse { data, offset },
                ))?;
                let _ = progress_tx.unbounded_send(ProgressState {
                    current: new_offset,
                    total: encrypted_file.len(),
                });
                Ok(Continue(()))
            }
            Some(downloader_message::Inner::TransferFinished(TransferFinished {})) => {
                debug!("outgoing RTC transfer finished");
                let _ = progress_tx.unbounded_send(ProgressState {
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

#[derive(Clone, Default)]
struct RelayRequestTimeoutHandle {
    handle: Arc<AtomicCell<Option<AbortHandle>>>,
}
const _ASSERT: () = debug_assert!(AtomicCell::<AtomicCell<Option<AbortHandle>>>::is_lock_free());

impl RelayRequestTimeoutHandle {
    fn put(&self, handle: AbortHandle) {
        self.handle.store(Some(handle));
    }

    fn cancel(&self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
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

        let uploader_client = UploaderClient::new_testing(Transport::Both);
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
