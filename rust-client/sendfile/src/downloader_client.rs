use std::io::Write;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use prost::Message;
use url::Url;

use crate::api_client::DecryptedFile;
use crate::cipher::ContentCipher;
use crate::p2p::protocol::{
    downloader_message, uploader_message, DataRequest, DataResponse, TransferFinished,
    UploaderMessage,
};
use crate::p2p::{PeerToPeerClient, PeerToPeerClientHandler};
use crate::websocket::web_socket_message;
use crate::{ApiClient, CipherKey, DownloadId, Error, Result};

pub struct DownloaderClient {
    api_client: ApiClient,
    download_id: DownloadId,
    output_dir: Option<PathBuf>,
}

const CHUNK_SIZE: u64 = 16384;

impl DownloaderClient {
    pub fn from_download_url(download_url_str: &str, api_endpoint: Url) -> Result<Self> {
        let (download_id, cipher_key) = Self::parse_download_url(download_url_str)?;

        let api_client = ApiClient::new(api_endpoint, cipher_key);
        Ok(Self { api_client, download_id, output_dir: None })
    }

    #[cfg(test)]
    pub fn from_testing_download_url(download_url_str: &str) -> Result<Self> {
        let api_endpoint = Url::parse("http://localhost:8080").expect("invalid hardcoded url");
        Self::from_download_url(download_url_str, api_endpoint)
    }

    #[cfg(test)]
    pub(crate) fn set_output_dir(&mut self, path: &Path) {
        self.output_dir = Some(path.into());
    }

    pub fn download(&self, p2p_timeout: Option<Duration>) -> Result<()> {
        match self.download_p2p(p2p_timeout) {
            Err(Error::Timeout) => self.download_relayed(),
            result => result,
        }
    }

    pub fn download_relayed(&self) -> Result<()> {
        let meta = self.api_client.fetch_meta(&self.download_id)?;
        let decrypted_file = self
            .api_client
            .decrypted_file(&meta, self.output_dir.as_deref());
        self.api_client.download_content(&meta, decrypted_file)
    }

    pub fn download_p2p(&self, timeout: Option<Duration>) -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let meta = self.api_client.fetch_meta(&self.download_id)?;
        let mut p2p_client = PeerToPeerClient::new()?;
        let signaling_message_handler = p2p_client.signaling_message_handler();
        let websocket = runtime.block_on(async {
            self.api_client
                .connect_download_websocket(&meta.encrypted_content_url, move |message| {
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
        let decrypted_file = self
            .api_client
            .decrypted_file(&meta, self.output_dir.as_deref());
        let mut state = DownloadState {
            inflight_data_request: false,
            decrypted_file,
            offset: 0,
            len: meta.file_meta.file_size + ContentCipher::extra_ciphertext_len(),
        };

        runtime.block_on(async {
            p2p_client.set_websocket(websocket).await?;
            p2p_client.create_offer().await?;
            p2p_client.transfer(&mut state, timeout).await
        })?;

        self.api_client.finish_download(&meta)?;
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

            if download_id_str.contains("/") {
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
}

impl PeerToPeerClientHandler for DownloadState<'_> {
    fn data_channel_opened(&mut self, client: &mut PeerToPeerClient) -> Result<ControlFlow<()>> {
        self.request_data_or_finish(client)
    }

    fn data_channel_message(
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
                    decrypted_file.write_all(&new_data)?;
                    *offset += len;
                    *inflight_data_request = false;
                } else {
                    warn!("received RTC DataResponse from uploader for unexpected offset {offset} len {len}");
                }
                self.request_data_or_finish(client)
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
    fn request_data_or_finish(&mut self, client: &mut PeerToPeerClient) -> Result<ControlFlow<()>> {
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
            decrypted_file.flush()?;
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
