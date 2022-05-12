use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use std::{io, mem};

use backoff::ExponentialBackoff;
use bytes::Bytes;
use futures::channel::mpsc;
use futures::{ready, AsyncRead, AsyncWrite, AsyncWriteExt, Future, FutureExt, StreamExt};
use reqwest::{header, Response, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;
use wasm_bindgen::prelude::*;

use crate::cipher::{CipherKey, ContentCipher, ContentCipherBuffer};
use crate::error::{AsRetriableResultExt, IntoResultExt, IntoRetriableResultExt};
use crate::util::{retry, ProgressState, TimeoutExt, TimeoutResult};
use crate::websocket::{WebSocketClient, WebSocketMessage};
use crate::{Error, Result};

// should this be configurable, or infinite even?
pub(crate) const CONTENT_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
        let buffer = ContentCipherBuffer::from_plaintext(metadata_json.as_bytes());
        let encrypted_metadata = self.cipher().encrypt_metadata(buffer).await;

        let encoded_metadata = base64::encode(encrypted_metadata);
        debug!(
            "posting to url: {}, encrypted_metadata: {:?}",
            url, &encoded_metadata
        );

        let form = [("encrypted_metadata", encoded_metadata)];

        let client = self.http_client();

        let backoff = ExponentialBackoff::default();
        let response = retry(backoff, || async {
            let request = client.post(url.clone()).form(&form);
            let result: TimeoutResult<_> = request.send().timeout(REQUEST_TIMEOUT).await;
            let result: reqwest::Result<_> = result.as_retriable_result()?;
            let response: Response = result.as_retriable_result()?;
            let response: Response = response.ok_or_retriable_err("failed to provision file")?;
            Ok::<_, backoff::Error<Error>>(response)
        })
        .await?;

        let provision_file_response = response.json::<ProvisionFileResponse>().await?;
        Ok(provision_file_response)
    }

    pub async fn encrypt_file<F: AsyncRead + Unpin>(
        &self,
        file: F,
        file_size: u64,
    ) -> io::Result<EncryptedFile> {
        let plaintext_len = file_size.try_into().expect("file fits in memory");
        let plaintext = ContentCipherBuffer::read_plaintext_to_end(file, plaintext_len).await?;
        // TODO - verify length matches that in metadata
        // assert_eq!(plaintext_len,

        // TODO stream
        let encrypted_bytes = self.cipher().encrypt_content(plaintext).await;
        Ok(EncryptedFile { encrypted_bytes: encrypted_bytes.into() })
    }

    pub async fn upload_file(&self, file: EncryptedFile, upload_path: &str) -> Result<()> {
        let url = self.endpoint.join(upload_path).expect("bad endpoint?");
        let start_url = {
            let mut start_url = url.clone();
            start_url
                .path_segments_mut()
                .map_err(|_| Error::InvalidInput("bad upload url"))?
                .push("start");
            start_url
        };
        let file_size = file.encrypted_bytes.len();
        let last_position = file_size - 1;

        let client = self.http_client();

        // We don't actually have concurrency here, because retry() awaits each future we return from the
        // closure before returning a new one, but the compiler doesn't know that.
        let position_shared: AtomicUsize = Default::default();
        let backoff = ExponentialBackoff::default();
        retry(backoff, || async {
            loop {
                let mut position = position_shared.load(Relaxed);

                let start_form = [("position", position)];
                let start_request = client.post(start_url.clone()).form(&start_form);

                let send_bytes = file.encrypted_bytes.slice(position..);
                let content_range = if position < file_size {
                    format!("bytes {position}-{last_position}/{file_size}")
                } else {
                    format!("bytes */{file_size}")
                };
                let request = client.post(url.clone()).body(send_bytes).header(header::CONTENT_RANGE, content_range);

                // First ask the server if we can start uploading content from this position.
                let response = match start_request.send().await.as_retriable_result()? {
                    start_response if !start_response.status().is_success() => {
                        // Fall through and handle this "start upload" error response in the same
                        // way as we handle content upload errors.
                        start_response
                    }
                    start_response => {
                        let response_bytes = start_response.bytes().await.as_retriable_result()?;

                        serde_json::from_slice::<UploadResponse>(&response_bytes).map_err(|error| {
                            error!("invalid server start upload response: {}", error);
                            backoff::Error::permanent(Error::InvalidServerResponse(
                                "Invalid start upload response",
                            ))
                        })?.ok_or_retriable_err("failed to start upload")?;

                        // Upload the file content.
                        //
                        // We can't have a request timeout here since we don't know whether the
                        // request has actually been accepted but we're just waiting to send data
                        // (or sending data just takes a long time). Upload request timeouts have to
                        // happen at a higher level, with help from feedback from the server via
                        // websocket.
                        request.send().await.as_retriable_result()?
                    }
                };

                if let StatusCode::CONFLICT = response.status() {
                    let response_bytes = response.bytes().await.as_retriable_result()?;
                    let conflict_response: UploadConflictResponse =
                        serde_json::from_slice(&response_bytes).map_err(|error| {
                            error!("invalid server 409 Conflict response: {}", error);
                            backoff::Error::permanent(Error::InvalidServerResponse(
                                "Invalid conflict error response",
                            ))
                        })?;
                    let old_position = position;
                    position = usize::try_from(conflict_response.position).expect("file fits in memory");
                    position_shared.store(position, Relaxed);
                    if position > file_size {
                        let error_message = format!(
                            "Downloader requested file position {position} \
                             which is greater than file size {file_size}.",
                        );
                        return Err(backoff::Error::permanent(Error::InvalidPeerMessage {
                            source: error_message.into(),
                        }));
                    } else if position == old_position {
                        // Return an error when the server returns a 409 for same offset that we started with.
                        // This isn't really an error, but we want to back off to be nice in case the server
                        // is malfunctioning.
                        warn!("server returned spurious 409 Conflict response with identical offset; \
                               backing off.");
                        break Err(backoff::Error::transient(Error::ClientHttpErrorResponse {
                            message: "Spurious 409 Conflict response from server; identical offset.",
                            status: StatusCode::CONFLICT.as_u16(),
                            retry_after: None,
                        }));
                    } else {
                        // fall through and retry
                    }
                } else {
                    let response = response
                        .ok_or_retriable_err("failed to upload content")?;
                    let response_bytes = response.bytes().await.as_retriable_result()?;

                    serde_json::from_slice::<UploadResponse>(&response_bytes).map_err(|error| {
                            error!("invalid server upload response: {}", error);
                            backoff::Error::permanent(Error::InvalidServerResponse(
                                "Invalid upload response",
                            ))
                    })?.ok_or_retriable_err("failed to upload")?;
                    break Ok(());
                }
            }
        })
        .await
    }

    pub async fn fetch_meta(&self, download_id: &DownloadId) -> Result<DownloadMeta> {
        let download_path = format!("/api/v1/download/{}", download_id);
        let url = self.endpoint.join(&download_path).expect("bad endpoint?");

        let client = self.http_client();

        let backoff = ExponentialBackoff::default();
        let response = retry(backoff, || async {
            let request = client.get(url.clone());
            let result: TimeoutResult<_> = request.send().timeout(REQUEST_TIMEOUT).await;
            let result: reqwest::Result<_> = result.as_retriable_result()?;
            let response: Response = result.as_retriable_result()?;
            let response: Response =
                response.ok_or_retriable_err("failed to fetch download details")?;
            Ok::<_, backoff::Error<Error>>(response)
        })
        .await?;

        #[derive(Debug, Deserialize, Serialize)]
        struct EncodedDownloadMeta {
            encrypted_content_url: String,
            meta: String,
        }
        let download_response = response.json::<EncodedDownloadMeta>().await?;
        debug!("download_response: {:?}", download_response);
        let decoded_metadata: Vec<u8> = base64::decode(&download_response.meta)
            .map_err(|_| Error::InvalidInput("invalid base64 encoding of metadata"))?;
        let decrypted_metadata = self.cipher().decrypt_metadata(decoded_metadata).await?;
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
        decrypted_file: DecryptedFile<'_>,
        progress_tx: mpsc::UnboundedSender<ProgressState<u64>>,
    ) -> Result<()> {
        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;
        let content_size = download_meta.file_meta.file_size;

        let client = self.http_client();

        let backoff = ExponentialBackoff::default();
        retry(backoff, || async {
            let content_offset = decrypted_file.offset();
            let request = client
                .get(content_url.clone())
                .header(header::RANGE, format!("bytes={content_offset}-"));
            let result: TimeoutResult<_> = request.send().timeout(REQUEST_TIMEOUT).await;
            let result: reqwest::Result<_> = result.as_retriable_result()?;
            let response: Response = result.as_retriable_result()?;
            let response: Response = response.ok_or_retriable_err("failed to download content")?;

            debug!("waiting on response body");
            let mut response_bytes_stream = response.bytes_stream();
            let mut content_downloaded = 0;
            let mut decrypted_file = decrypted_file.clone();
            while let Some(data) = response_bytes_stream
                .next()
                .timeout(CONTENT_TIMEOUT)
                .await
                .as_retriable_result()?
                .transpose()
                .as_retriable_result()?
            {
                content_downloaded += u64::try_from(data.len()).expect("128-bit machine?");
                let _ignore = progress_tx.unbounded_send(ProgressState {
                    current: content_offset + content_downloaded,
                    total: content_size,
                });
                let () = decrypted_file
                    .write_all(&data)
                    .await
                    .map_err(|error| backoff::Error::permanent(error.into()))?;
            }
            let () = decrypted_file
                .flush()
                .await
                .map_err(|error| backoff::Error::permanent(error.into()))?;
            let () = decrypted_file
                .close()
                .await
                .map_err(|error| backoff::Error::permanent(error.into()))?;

            Ok::<_, backoff::Error<Error>>(())
        })
        .await
    }

    pub async fn finish_download(&self, download_meta: &DownloadMeta) -> Result<()> {
        // Finishing a download through the relay currently just means sending a ranged download
        // request where the requested offset is the end of the file. This essentially tells the
        // server that we have downloaded the entirety of the data.

        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;

        let client = self.http_client();

        let backoff = ExponentialBackoff::default();
        retry(backoff, || async {
            let request = client
                .get(content_url.clone())
                .header(header::RANGE, format!("bytes=-0"));
            let result: TimeoutResult<_> = request.send().timeout(REQUEST_TIMEOUT).await;
            let result: reqwest::Result<_> = result.as_retriable_result()?;
            let response: Response = result.as_retriable_result()?;
            if let StatusCode::NOT_FOUND = response.status() {
                // The session was already terminated; treat this as a success.
                Ok::<_, backoff::Error<Error>>(())
            } else {
                response.ok_or_retriable_err("failed to finish download")?;
                Ok(())
            }
        })
        .await?;

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
            !id.contains('/'),
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

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum UploadResponse {
    Ok,
    Error { reason: String },
}

impl IntoResultExt for UploadResponse {
    type Output = ();
    fn ok_or(self, message: &'static str) -> Result<Self::Output> {
        match self {
            Self::Ok => Ok(()),
            Self::Error { reason } => Err(Error::ClientApiErrorResponse { message, reason }),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct UploadConflictResponse {
    pub(crate) position: u64,
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

    pub(crate) fn len(&self) -> u64 {
        u64::try_from(self.encrypted_bytes.len()).unwrap()
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

impl DecryptedFile<'_> {
    pub(crate) fn offset(&self) -> u64 {
        let shared = self.shared.lock().unwrap();
        match &shared.state {
            DecryptedFileWriteState::Unwritten { data, .. } => {
                data.len().try_into().expect("file fits in memory")
            }
            DecryptedFileWriteState::Pending { .. } | DecryptedFileWriteState::Complete => {
                self.file_size
            }
            DecryptedFileWriteState::Poisoned => panic!("invalid state"),
        }
    }
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
                            .decrypt_content(data)
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
