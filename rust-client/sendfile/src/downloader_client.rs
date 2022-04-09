#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use futures::{AsyncWriteExt, Future};
use prost::Message;
use url::Url;

use crate::api_client::{DecryptedFile, DownloadMeta};
use crate::cipher::ContentCipher;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, TransferFinished,
    UploaderMessage,
};
use crate::p2p::{PeerToPeerClient, PeerToPeerClientHandler};
use crate::util::{Progress, ProgressState};
use crate::websocket::web_socket_message;
use crate::{mpsc, ApiClient, CipherKey, DownloadId, Error, Result, Transport};

pub struct DownloaderClient {
    api_client: ApiClient,
    download_id: DownloadId,
    #[cfg_attr(target_arch = "wasm32", allow(unused))]
    output_dir: Option<PathBuf>,
    transport: Transport,
}

const CHUNK_SIZE: u64 = 16384;

impl DownloaderClient {
    pub fn from_download_url(
        download_url_str: &str,
        api_endpoint: Url,
        transport: Transport,
    ) -> Result<Self> {
        let (download_id, cipher_key) = Self::parse_download_url(download_url_str)?;

        let api_client = ApiClient::new(api_endpoint, cipher_key);
        Ok(Self { api_client, download_id, output_dir: None, transport })
    }

    #[cfg(test)]
    pub fn from_testing_download_url(download_url_str: &str, transport: Transport) -> Result<Self> {
        let api_endpoint = option_env!("TEST_SENDFILE_API_ENDPOINT")
            .unwrap_or("http://localhost:8080")
            .parse()
            .expect("invalid hardcoded url");
        Self::from_download_url(download_url_str, api_endpoint, transport)
    }

    #[cfg(test)]
    pub(crate) fn set_output_dir(&mut self, path: &Path) {
        self.output_dir = Some(path.into());
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
            match self.transport {
                Transport::Both => match self
                    .download_p2p_async(
                        meta,
                        decrypted_file.clone(),
                        p2p_timeout,
                        progress_tx.clone(),
                    )
                    .await
                {
                    Err(Error::Timeout) => {
                        self.download_relayed_async(meta, decrypted_file, progress_tx)
                            .await
                    }
                    result => result,
                },
                Transport::P2P => {
                    self.download_p2p_async(meta, decrypted_file, p2p_timeout, progress_tx)
                        .await
                }
                Transport::Relay => {
                    self.download_relayed_async(meta, decrypted_file, progress_tx)
                        .await
                }
            }
        })
    }

    async fn download_relayed_async(
        &self,
        meta: &DownloadMeta,
        decrypted_file: DecryptedFile<'_>,
        progress_tx: mpsc::Sender<ProgressState<u64>>,
    ) -> Result<()> {
        let result = self
            .api_client
            .download_content(meta, decrypted_file, progress_tx)
            .await?;
        info!("successfully completed relayed file transfer");
        Ok(result)
    }

    async fn download_p2p_async(
        &self,
        meta: &DownloadMeta,
        decrypted_file: DecryptedFile<'_>,
        timeout: Option<Duration>,
        progress_tx: mpsc::Sender<ProgressState<u64>>,
    ) -> Result<()> {
        let mut p2p_client = PeerToPeerClient::new()?;
        let signaling_message_handler = p2p_client.signaling_message_handler();
        let websocket = self
            .api_client
            .connect_download_websocket(&meta.encrypted_content_url, move |message| {
                match message.inner {
                    Some(web_socket_message::Inner::RtcSignaling(message)) => {
                        signaling_message_handler
                            .handle(message)
                            .map(Continue)
                            .unwrap_or(Break(()))
                    }
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
            })
            .await?;
        let mut state = DownloadState {
            inflight_data_request: false,
            decrypted_file,
            progress_tx,
            offset: 0,
            len: meta.file_meta.file_size + ContentCipher::extra_ciphertext_len(),
        };

        p2p_client.set_websocket(websocket).await?;
        p2p_client.create_offer().await?;
        p2p_client.transfer(&mut state, timeout).await?;

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

struct DownloadState<'a> {
    inflight_data_request: bool,
    decrypted_file: DecryptedFile<'a>,
    offset: u64,
    len: u64,
    progress_tx: mpsc::Sender<ProgressState<u64>>,
}

#[async_trait::async_trait(?Send)]
impl PeerToPeerClientHandler for DownloadState<'_> {
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
        let Self { inflight_data_request, decrypted_file, offset, .. } = self;
        let message = UploaderMessage::decode(message_data).map_err(Error::rtc_err)?;
        match message.inner {
            Some(uploader_message::Inner::DataResponse(DataResponse {
                offset: new_data_offset,
                data: new_data,
            })) => {
                let len = u64::try_from(new_data.len()).expect("128-bit machine??");
                if *offset == new_data_offset {
                    debug!("received RTC DataResponse from uploader for offset {offset} len {len}");
                    decrypted_file.write_all(&new_data).await?;
                    *offset += len;
                    *inflight_data_request = false;
                    let _ignore = self
                        .progress_tx
                        .send(ProgressState { current: *offset, total: self.len });
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
}

impl DownloadState<'_> {
    async fn request_data_or_finish(
        &mut self,
        client: &mut PeerToPeerClient,
    ) -> Result<ControlFlow<()>> {
        let Self { inflight_data_request, decrypted_file, offset, len, .. } = self;
        let chunk_len = (*len - *offset).min(CHUNK_SIZE);
        if chunk_len != 0 {
            if !*inflight_data_request {
                debug!("sending RTC DataRequest to uploader for offset {offset} len {chunk_len}");
                client.send_downloader_message(downloader_message::Inner::DataRequest(
                    DataRequest { offset: *offset, len: chunk_len },
                ))?;
                *inflight_data_request = true;
            }
            Ok(Continue(()))
        } else {
            debug!("incoming RTC transfer finished");
            decrypted_file.flush().await?;
            client.send_downloader_message(downloader_message::Inner::TransferFinished(
                TransferFinished {},
            ))?;
            Ok(Break(()))
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
