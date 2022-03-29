use std::ops::ControlFlow;

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::{io, mem};

use bytes::Bytes;
use cfg_if::cfg_if;
use futures::{
    pin_mut, ready, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, Future, FutureExt, SinkExt,
    StreamExt, TryStreamExt,
};
use serde::{Deserialize, Serialize};
use url::Url;
use wasm_bindgen::prelude::*;

use crate::cipher::{CipherKey, ContentCipher};
use crate::websocket::{WebSocketClient, WebSocketMessage};
use crate::{Error, Result};

// should this be configurable, or infinite even?
#[cfg_attr(target_arch = "wasm32", allow(unused))]
const CONTENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(500);

pub(crate) struct ApiClient {
    cipher_key: CipherKey,
    endpoint: Url,
}

impl ApiClient {
    pub fn new(endpoint: Url, cipher_key: CipherKey) -> Self {
        Self { cipher_key, endpoint }
    }

    fn cipher(&self) -> ContentCipher {
        ContentCipher::new(&self.cipher_key)
    }

    pub(crate) fn cipher_key(&self) -> &CipherKey {
        &self.cipher_key
    }

    pub async fn provision_file(
        &self,
        file_name: String,
        file_size: u64,
    ) -> Result<ProvisionFileResponse> {
        let url = self.endpoint.join("/api/v1/files").expect("bad endpoint?");

        let file_meta = FileMeta { file_name, file_size };
        let metadata_json = serde_json::to_string(&file_meta)
            .map_err(|_| Error::InvalidInput("unserializable upload"))?;
        let encrypted_metadata = self.cipher().encrypt(metadata_json.as_bytes()).await;

        let encoded_metadata = base64::encode(encrypted_metadata);
        debug!(
            "posting to url: {}, encrypted_metadata: {:?}",
            url, &encoded_metadata
        );

        let form = [("encrypted_metadata", encoded_metadata)];

        let response = self.http_client().post(url).form(&form).send().await?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to provision file",
                status: response.status().as_u16(),
            });
        }

        let provision_file_response = response.json::<ProvisionFileResponse>().await?;
        Ok(provision_file_response)
    }

    pub async fn encrypt_file<F: AsyncRead + 'static>(&self, file: F) -> EncryptedFile {
        let mut plaintext = vec![];
        pin_mut!(file);
        let _plaintext_len = file.read_to_end(&mut plaintext).await;
        // TODO - verify length matches that in metadata
        // assert_eq!(plaintext_len,

        // TODO stream
        let encrypted_bytes = self.cipher().encrypt(&plaintext).await;
        EncryptedFile { encrypted_bytes: encrypted_bytes.into() }
    }

    pub async fn upload_file(&self, file: EncryptedFile, upload_path: &str) -> Result<()> {
        let url = self.endpoint.join(upload_path).expect("bad endpoint?");

        let mut client_builder = self.http_client_builder();

        cfg_if::cfg_if! {
            if #[cfg(not(target_arch = "wasm32"))] {
                client_builder = client_builder.timeout(CONTENT_TIMEOUT);
            } else {
                client_builder = client_builder;
            }
        }
        let response = client_builder
            .build()
            .expect("invalid timeout for http client?")
            .post(url)
            .body(file.encrypted_bytes)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to upload content",
                status: response.status().as_u16(),
            });
        }

        Ok(())
    }

    pub async fn fetch_meta(&self, download_id: &DownloadId) -> Result<DownloadMeta> {
        let download_path = format!("/api/v1/download/{}", download_id);
        let url = self.endpoint.join(&download_path).expect("bad endpoint?");

        let response = self.http_client().get(url).send().await?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to fetch download details",
                status: response.status().as_u16(),
            });
        }

        #[derive(Debug, Deserialize, Serialize)]
        struct EncodedDownloadMeta {
            encrypted_content_url: String,
            meta: String,
        }
        let download_response = response.json::<EncodedDownloadMeta>().await?;
        debug!("download_response: {:?}", download_response);
        let decoded_metadata: Vec<u8> = base64::decode(&download_response.meta)
            .map_err(|_| Error::InvalidInput("invalid base64 encoding of metadata"))?;
        let decrypted_metadata = self.cipher().decrypt(&decoded_metadata).await?;
        let file_meta = FileMeta::try_from_encoded(&decrypted_metadata)?;
        Ok(DownloadMeta {
            encrypted_content_url: download_response.encrypted_content_url,
            file_meta,
        })
    }

    pub fn decrypt_file<F: AsyncWrite + 'static>(
        &self,
        download_meta: &DownloadMeta,
        file: F,
    ) -> DecryptedFile<'_> {
        let data_len =
            usize::try_from(download_meta.file_meta.file_size).expect("file fits in memory");
        DecryptedFile {
            api_client: self,
            file_size: download_meta.file_meta.file_size + ContentCipher::extra_ciphertext_len(),
            shared: Arc::new(Mutex::new(DecryptedFileShared {
                state: DecryptedFileWriteState::Unwritten {
                    data: Vec::with_capacity(data_len),
                    file: Box::pin(file),
                },
            })),
        }
    }

    pub async fn download_content(
        &self,
        download_meta: &DownloadMeta,
        mut decrypted_file: DecryptedFile<'_>,
    ) -> Result<()> {
        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;

        let mut client_builder = self.http_client_builder();

        cfg_if::cfg_if! {
            if #[cfg(not(target_arch = "wasm32"))] {
                client_builder = client_builder.timeout(CONTENT_TIMEOUT);
            } else {
                client_builder = client_builder;
            }
        }

        let response = client_builder
            .build()
            .expect("invalid timeout for http client?")
            .get(content_url)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to download content",
                status: response.status().as_u16(),
            });
        } else {
            debug!("received successful response headers");
        }

        debug!("waiting on response body");
        let response_bytes_stream = {
            cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    // TODO contribute reqwest::Response::bytes_stream upstream
                    futures::stream::once(response.bytes()).map_err(Error::from)
                } else {
                    response.bytes_stream().map_err(Error::from)
                }
            }
        };
        let mut decrypted_file_sink = Pin::new(&mut decrypted_file)
            .into_sink()
            .sink_map_err(Error::from);

        response_bytes_stream
            .forward(&mut decrypted_file_sink)
            .await?;
        decrypted_file_sink.close().await?;

        Ok(())
    }

    pub async fn finish_download(&self, download_meta: &DownloadMeta) -> Result<()> {
        // Finishing a download through the relay currently just means sending a ranged download
        // request where the requested offset is the end of the file. This essentially tells the
        // server that we have downloaded the entirety of the data.

        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;
        let content_size = download_meta.file_meta.file_size;

        let request = self.http_client().get(content_url);
        let request = request.header("Content-Range", format!("bytes {content_size}-/*"));

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to finish download",
                status: response.status().as_u16(),
            });
        }

        Ok(())
    }

    pub async fn connect_download_websocket(
        &self,
        encrypted_content_url: &str,
        handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<WebSocketClient> {
        let websocket_url = {
            let mut content_url = self
                .endpoint
                .join(encrypted_content_url)
                .map_err(|_| Error::InvalidInput("bad content url"))?;
            content_url
                .path_segments_mut()
                .map_err(|_| Error::InvalidInput("bad content url"))?
                .pop()
                .push("ws");
            match content_url.scheme() {
                "https" => content_url.set_scheme("wss").expect("bad scheme?"),
                "http" => content_url.set_scheme("ws").expect("bad scheme?"),
                _ => return Err(Error::InvalidInput("bad api url scheme")),
            }
            content_url
        };
        let websocket_client =
            WebSocketClient::connect(websocket_url.as_str(), handle_incoming_message).await?;
        Ok(websocket_client)
    }

    pub async fn connect_upload_websocket(
        &self,
        upload_path: &str,
        handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<WebSocketClient> {
        let websocket_url = {
            let mut upload_url = self
                .endpoint
                .join(upload_path)
                .map_err(|_| Error::InvalidInput("bad upload path"))?;
            upload_url
                .path_segments_mut()
                .map_err(|_| Error::InvalidInput("bad upload path"))?
                .push("ws");
            match upload_url.scheme() {
                "https" => upload_url.set_scheme("wss").expect("bad scheme?"),
                "http" => upload_url.set_scheme("ws").expect("bad scheme?"),
                _ => return Err(Error::InvalidInput("bad api url scheme")),
            }
            upload_url
        };
        let websocket_client =
            WebSocketClient::connect(websocket_url.as_str(), handle_incoming_message).await?;
        Ok(websocket_client)
    }

    fn http_client_builder(&self) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
    }

    fn http_client(&self) -> reqwest::Client {
        self.http_client_builder()
            .build()
            .expect("invalid default http client config")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct DownloadId(String);

impl std::fmt::Display for DownloadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl DownloadId {
    pub fn new(id: String) -> Self {
        assert!(
            !id.contains("/"),
            "'id' looks like a path: {}. Improperly parsed?",
            id
        );
        Self(id)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProvisionFileResponse {
    pub(crate) upload_url: String,
    pub(crate) download_id: String,
}

#[derive(Clone, Debug)]
#[wasm_bindgen(getter_with_clone)]
pub struct DownloadMeta {
    pub(crate) encrypted_content_url: String,
    #[wasm_bindgen(js_name = fileMeta)]
    pub file_meta: FileMeta,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[wasm_bindgen(getter_with_clone)]
pub struct FileMeta {
    #[wasm_bindgen(js_name = fileName)]
    pub file_name: String,
    #[wasm_bindgen(js_name = fileSize)]
    pub file_size: u64,
}

impl FileMeta {
    fn try_from_encoded(bytes: &[u8]) -> Result<Self> {
        let string = String::from_utf8(bytes.to_vec())
            .map_err(|_| Error::InvalidInput("invalid unicode in FileMeta serialization"))?;
        serde_json::from_str::<FileMeta>(&string).map_err(|_| {
            error!("invalid json string: {}", &string);
            Error::InvalidInput("Invalid json in FileMeta serialization")
        })
    }
}

#[derive(Clone)]
pub(crate) struct EncryptedFile {
    encrypted_bytes: Bytes,
}

impl EncryptedFile {
    pub(crate) fn read_at_exact(&self, offset: u64, len: usize) -> Bytes {
        let offset = usize::try_from(offset).expect("file fits in memory");
        self.encrypted_bytes
            .slice(offset..offset.saturating_add(len))
    }
}

#[derive(Clone)]
pub(crate) struct DecryptedFile<'a> {
    api_client: &'a ApiClient,
    file_size: u64,
    shared: Arc<Mutex<DecryptedFileShared>>,
}

struct DecryptedFileShared {
    state: DecryptedFileWriteState,
}

enum DecryptedFileWriteState {
    Unwritten {
        data: Vec<u8>,
        file: Pin<Box<dyn AsyncWrite>>,
    },
    Pending {
        pending_write: Pin<Box<dyn Future<Output = io::Result<()>>>>,
    },
    Complete,
    Poisoned,
}

impl AsyncWrite for DecryptedFile<'_> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // TODO: stream rather
        match &mut self.shared.lock().unwrap().state {
            DecryptedFileWriteState::Unwritten { data, .. } => {
                data.extend(buf);
                Poll::Ready(Ok(buf.len()))
            }
            DecryptedFileWriteState::Pending { .. } | DecryptedFileWriteState::Complete { .. } => {
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "received more bytes than expected",
                )))
            }
            DecryptedFileWriteState::Poisoned => panic!("invalid state"),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let file_size = usize::try_from(self.file_size).expect("file fits in memory");

        let mut shared = self.shared.lock().unwrap();
        let mut pending_write =
            match mem::replace(&mut shared.state, DecryptedFileWriteState::Poisoned) {
                DecryptedFileWriteState::Unwritten { mut file, data } => {
                    if data.len() != file_size {
                        // It isn't great to return success here and silently ignore the fact that we weren't able to
                        // flush anything, but we should have streaming encryption soon and this won't happen then.
                        shared.state = DecryptedFileWriteState::Unwritten { file, data };
                        return Poll::Ready(Ok(()));
                    }

                    let cipher = self.api_client.cipher();
                    Box::pin(async move {
                        let plaintext = cipher
                            .decrypt(&data)
                            .await
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

                        file.write_all(&plaintext).await?;
                        file.flush().await?;
                        file.close().await?;
                        Ok(())
                    })
                }
                DecryptedFileWriteState::Pending { pending_write } => pending_write,
                DecryptedFileWriteState::Complete => {
                    shared.state = DecryptedFileWriteState::Complete;
                    return Poll::Ready(Ok(()));
                }
                DecryptedFileWriteState::Poisoned => panic!("invalid state"),
            };

        match pending_write.poll_unpin(cx)? {
            Poll::Pending => {
                shared.state = DecryptedFileWriteState::Pending { pending_write };
                Poll::Pending
            }
            Poll::Ready(()) => {
                shared.state = DecryptedFileWriteState::Complete;
                Poll::Ready(Ok(()))
            }
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_flush(cx))?;
        let shared = self.shared.lock().unwrap();
        match &shared.state {
            DecryptedFileWriteState::Complete => Poll::Ready(Ok(())),
            DecryptedFileWriteState::Pending { .. } | DecryptedFileWriteState::Unwritten { .. } => {
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "received less bytes than expected",
                )))
            }
            DecryptedFileWriteState::Poisoned => panic!("invalid state"),
        }
    }
}
