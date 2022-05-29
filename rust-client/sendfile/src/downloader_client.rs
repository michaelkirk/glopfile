#[cfg(all(feature = "ffi", not(target_arch = "wasm32")))]
mod ffi;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};
use std::time::Duration;

use bytes::Bytes;
use futures::channel::mpsc;
use futures::{pin_mut, AsyncWriteExt, Future, FutureExt};
use prost::Message;
use url::Url;

use crate::api_client::{DecryptedFile, DownloadMeta};
use crate::cipher::ContentCipher;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, DownloaderHello,
    TransferFinished, UploaderHello, UploaderMessage,
};
use crate::p2p::{
    PeerToPeerClient, PeerToPeerClientHandler, SignalingCipherUsage, SignalingMessageHandler,
};
use crate::util::{Progress, ProgressState};
use crate::websocket::{web_socket_message, WebSocketMessageHandler};
use crate::{ApiClient, CipherKey, DownloadId, Error, Result, Transport};

pub struct DownloaderClient {
    api_client: ApiClient,
    download_id: DownloadId,
    transport: Transport,
}

struct PeerToPeerConnectHandler<'a, 'b> {
    state: &'a mut DownloadState<'b>,
}

const CHUNK_SIZE: u64 = 16 * 1024;
const MAX_P2P_INFLIGHT_DATA_LEN: u64 = 10 * 1024 * 1024;

impl DownloaderClient {
    pub fn from_download_url(
        download_url_str: &str,
        api_endpoint: Url,
        transport: Transport,
    ) -> Result<Self> {
        let (download_id, cipher_key) = Self::parse_download_url(download_url_str)?;

        let api_client = ApiClient::new(api_endpoint, cipher_key);
        Ok(Self { api_client, download_id, transport })
    }

    #[cfg(test)]
    pub fn from_testing_download_url(download_url_str: &str, transport: Transport) -> Result<Self> {
        let api_endpoint = option_env!("TEST_SENDFILE_API_ENDPOINT")
            .unwrap_or("http://localhost:8080")
            .parse()
            .expect("invalid hardcoded url");
        Self::from_download_url(download_url_str, api_endpoint, transport)
    }

    pub fn download_id(&self) -> &DownloadId {
        &self.download_id
    }

    async fn fetch_meta_async(&self) -> Result<DownloadMeta> {
        self.api_client.fetch_meta(&self.download_id).await
    }

    fn download_async<'a>(
        &'a self,
        meta: &'a DownloadMeta,
        decrypted_file: DecryptedFile<'a>,
        p2p_timeout: Option<Duration>,
    ) -> Progress<u64, impl Future<Output = Result<()>> + 'a> {
        Progress::new_with(|progress_tx| async move {
            let mut state = DownloadState {
                decrypted_file,
                progress_tx,
                requested_offset: 0,
                total_len: meta.file_meta.file_size + ContentCipher::extra_ciphertext_len(),
            };
            match self.transport {
                Transport::Both => self.download_p2p(meta, &mut state, p2p_timeout).await,
                Transport::P2P => self.download_p2p(meta, &mut state, None).await,
                Transport::Relay => {
                    self.download_relayed(meta, state.decrypted_file, state.progress_tx)
                        .await
                }
            }
        })
    }

    async fn download_p2p(
        &self,
        meta: &DownloadMeta,
        state: &mut DownloadState<'_>,
        p2p_timeout: Option<Duration>,
    ) -> Result<()> {
        let mut p2p_client = PeerToPeerClient::new(
            self.api_client.cipher_key(),
            SignalingCipherUsage::Downloader,
        )?;

        let websocket_message_handler = DownloadWebSocketMessageHandler {
            signaling_message_handler: p2p_client.signaling_message_handler(),
        };

        let websocket_client = self
            .api_client
            .connect_download_websocket(&meta.encrypted_content_url, websocket_message_handler)?;

        p2p_client.set_websocket_client(websocket_client);

        // First try one transfer p2p-only (to save relay server bandwidth) until the first error or timeout.
        let p2p_result = match Self::connect_p2p(&mut p2p_client, state, p2p_timeout).await {
            Ok(Continue(())) => {
                self.transfer_p2p(&mut p2p_client, meta, state, p2p_timeout)
                    .await
            }
            Ok(Break(())) => return Ok(()),
            Err(error) => Err(error),
        };

        match p2p_result {
            Ok(()) => return Ok(()),
            Err(error) => match self.transport {
                // If user requested fallback to relayed transfer, continue below.
                Transport::Both => match error {
                    Error::Timeout => {
                        info!("p2p transfer timed out; falling back to relayed transfer")
                    }
                    _ => warn!("p2p transfer error; falling back to relayed transfer: {error}"),
                },

                // If user requested p2p-only, return the error
                Transport::P2P => {
                    warn!("p2p transfer error: {error}");
                    return Err(error);
                }

                Transport::Relay => unreachable!(),
            },
        }

        loop {
            // Fall back to relayed while still attempting p2p.
            let relayed_task = self
                .download_relayed(
                    meta,
                    state.decrypted_file.clone(),
                    state.progress_tx.clone(),
                )
                .fuse();

            // Try to exchange hellos over the p2p channel concurrently while performing the relayed
            // transfer.
            let connect_p2p_task = Self::connect_p2p(&mut p2p_client, state, None).fuse();

            {
                pin_mut!(connect_p2p_task, relayed_task);
                futures::select! {
                    connect_p2p_result = connect_p2p_task => match connect_p2p_result {
                        Ok(Continue(())) => {
                            // Make sure the relayed transfer terminates.
                            drop(relayed_task);
                        }
                        Ok(Break(())) => break Ok(()),
                        Err(error) => {
                            // Finish with relayed transfer
                            warn!("p2p transfer error; continuing relayed transfer: {error}");
                            break relayed_task.await;
                        }
                    },
                    relayed_result = relayed_task => break relayed_result,
                }
            }

            // Continue transfer over p2p until we hit another error or timeout.
            match self
                .transfer_p2p(&mut p2p_client, meta, state, p2p_timeout)
                .await
            {
                Ok(()) => break Ok(()),
                Err(Error::Timeout) => info!("p2p transfer timed out; falling back to relayed"),
                Err(error) => warn!("p2p error; falling back to relayed: {error}"),
            }
        }
    }

    async fn download_relayed(
        &self,
        meta: &DownloadMeta,
        decrypted_file: DecryptedFile<'_>,
        progress_tx: mpsc::UnboundedSender<ProgressState<u64>>,
    ) -> Result<()> {
        let result = self
            .api_client
            .download_content(meta, decrypted_file, progress_tx)
            .await?;
        info!("successfully completed relayed file transfer");
        Ok(result)
    }

    async fn connect_p2p(
        p2p_client: &mut PeerToPeerClient,
        state: &mut DownloadState<'_>,
        timeout: Option<Duration>,
    ) -> Result<ControlFlow<()>> {
        p2p_client.create_offer().await?;
        let mut connect_handler = PeerToPeerConnectHandler { state };
        p2p_client.transfer(&mut connect_handler, timeout).await
    }

    async fn transfer_p2p(
        &self,
        p2p_client: &mut PeerToPeerClient,
        meta: &DownloadMeta,
        state: &mut DownloadState<'_>,
        timeout: Option<Duration>,
    ) -> Result<()> {
        p2p_client.transfer(state, timeout).await?;

        info!("successfully completed p2p file transfer");
        self.api_client.finish_download(meta).await?;
        Ok(())
    }

    fn parse_download_url(download_url_str: &str) -> Result<(DownloadId, CipherKey)> {
        let download_url = Url::parse(download_url_str)
            .map_err(|_| Error::InvalidInput("unparseable download url"))?;

        let cipher_key = {
            let fragment = download_url
                .fragment()
                .ok_or(Error::InvalidInput("download url was missing fragment"))?;
            let serialized = fragment
                .strip_prefix("cipher_key=")
                .ok_or(Error::InvalidInput(
                    "download url cipher_key was invalid or missing",
                ))?;
            CipherKey::from_string(serialized)?
        };
        // Note: we don't actually do anything with the download link _host_. It's only needed for the web
        // client. The rust client only communicates with the configured API service.

        let download_id = {
            let download_path = download_url.path().to_string();
            let download_id_str = download_path.strip_prefix("/download/").ok_or_else(|| {
                error!("invalid download_path: {}", download_path);
                Error::InvalidInput("Invalid download url")
            })?;

            if download_id_str.contains('/') {
                error!("invalid download_id containing '/': {}", download_path);
                return Err(Error::InvalidInput("Invalid download url"));
            }

            DownloadId::new(download_id_str.to_string())
        };

        Ok((download_id, cipher_key))
    }
}

#[derive(Clone)]
struct DownloadWebSocketMessageHandler {
    signaling_message_handler: SignalingMessageHandler,
}

impl WebSocketMessageHandler for DownloadWebSocketMessageHandler {
    fn handle(&mut self, message: crate::websocket::WebSocketMessage) -> ControlFlow<()> {
        match message.inner {
            Some(web_socket_message::Inner::RtcSignaling(message)) => self
                .signaling_message_handler
                .handle(message)
                .map(Continue)
                .unwrap_or(Break(())),
            Some(message @ web_socket_message::Inner::UploadDataAck(_)) => {
                warn!("unexpected websocket message: {message:?}");
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

#[async_trait::async_trait(?Send)]
impl PeerToPeerClientHandler for PeerToPeerConnectHandler<'_, '_> {
    type Output = ControlFlow<()>;

    async fn data_channel_opened(
        &mut self,
        client: &mut PeerToPeerClient,
    ) -> Result<ControlFlow<Self::Output>> {
        client.send_downloader_message(downloader_message::Inner::Hello(DownloaderHello {}))?;
        Ok(Continue(()))
    }

    async fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: Bytes,
    ) -> Result<ControlFlow<Self::Output>> {
        let output = match self
            .state
            .data_channel_message(client, message_data)
            .await?
        {
            Continue(()) => self.state.request_data_or_finish(client).await?,
            Break(()) => Break(()),
        };
        Ok(Break(output))
    }
}

struct DownloadState<'a> {
    decrypted_file: DecryptedFile<'a>,
    requested_offset: u64,
    total_len: u64,
    progress_tx: mpsc::UnboundedSender<ProgressState<u64>>,
}

#[async_trait::async_trait(?Send)]
impl PeerToPeerClientHandler for DownloadState<'_> {
    type Output = ();

    async fn data_channel_opened(
        &mut self,
        client: &mut PeerToPeerClient,
    ) -> Result<ControlFlow<()>> {
        self.request_data_or_finish(client).await
    }

    async fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>> {
        let message = UploaderMessage::decode(message_data).map_err(Error::rtc_err)?;
        self.handle_uploader_message(client, message).await
    }
}

impl DownloadState<'_> {
    async fn handle_uploader_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message: UploaderMessage,
    ) -> Result<ControlFlow<()>> {
        let Self { decrypted_file, .. } = self;
        match message.inner {
            Some(uploader_message::Inner::Hello(UploaderHello {})) => Ok(Continue(())),
            Some(uploader_message::Inner::DataResponse(DataResponse { offset, data })) => {
                let received_offset = decrypted_file.offset();
                let len = u64::try_from(data.len()).expect("128-bit machine??");
                if received_offset == offset {
                    debug!("received RTC DataResponse from uploader for offset {offset} len {len}");
                    decrypted_file.write_all(&data).await?;
                    let _ignore = self.progress_tx.unbounded_send(ProgressState {
                        current: received_offset + len,
                        total: self.total_len,
                    });
                } else {
                    warn!("received RTC DataResponse from uploader for unexpected offset {offset} len {len}");
                }
                self.request_data_or_finish(client).await
            }
            None => {
                // Unfortunately, with prost there's no way to log about what message type this actually was.
                warn!("unhandled RTC data channel message type from uploader");
                Ok(Continue(()))
            }
        }
    }

    async fn request_data_or_finish(
        &mut self,
        client: &mut PeerToPeerClient,
    ) -> Result<ControlFlow<()>> {
        let received_offset = self.decrypted_file.offset();
        if received_offset < self.total_len {
            while let Some(request @ DataRequest { offset: request_offset, len: request_len }) =
                self.next_data_request(received_offset)
            {
                debug!("sending RTC DataRequest to uploader for offset {request_offset} len {request_len}");
                client.send_downloader_message(downloader_message::Inner::DataRequest(request))?;
                self.requested_offset += request_len;
            }
            Ok(Continue(()))
        } else {
            debug!("incoming RTC transfer finished");
            self.decrypted_file.flush().await?;
            client.send_downloader_message(downloader_message::Inner::TransferFinished(
                TransferFinished {},
            ))?;
            Ok(Break(()))
        }
    }

    fn next_data_request(&self, received_offset: u64) -> Option<DataRequest> {
        let Self { requested_offset, total_len, .. } = self;
        let inflight_len = requested_offset.checked_sub(received_offset);
        let inflight_len = inflight_len.expect("requested_offset < received_offset");
        let unrequested_len = *total_len - *requested_offset;
        let request_len = MAX_P2P_INFLIGHT_DATA_LEN
            .saturating_sub(inflight_len)
            .min(unrequested_len)
            .min(CHUNK_SIZE);

        if request_len != 0 {
            let data_request = DataRequest { offset: *requested_offset, len: request_len };
            Some(data_request)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::downloader_client::*;

    #[test]
    fn test_parse_download_url() {
        let download_url =
            "https://foo.bar:123/download/123abc#cipher_key=AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

        let (download_id, key) = DownloaderClient::parse_download_url(download_url).unwrap();

        assert_eq!(download_id, DownloadId::new("123abc".to_string()));
        assert_eq!(key.bytes(), &[1u8; 32]);
    }
}
