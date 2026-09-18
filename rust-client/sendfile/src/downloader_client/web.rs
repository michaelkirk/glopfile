use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use futures::{pin_mut, ready, AsyncWrite, Future, FutureExt, TryStreamExt};
use std::time::Duration;
use js_sys::{JsString, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::DownloaderClient;
use crate::api_client::DownloadMeta;
use crate::transport::Transport;

#[wasm_bindgen(typescript_custom_section)]
const DOWNLOADABLE_FILE_INTERFACE: &'static str = r#"
export interface DownloadableFile {
    async write(data: Uint8Array): Promise<void>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "DownloadableFile")]
    pub type JsDownloadableFile;

    #[wasm_bindgen(method, catch)]
    async fn write(this: &JsDownloadableFile, data: Uint8Array) -> Result<(), js_sys::Error>;
}

#[wasm_bindgen(typescript_custom_section)]
const DOWNLOAD_EVENT_HANDLER_INTERFACE: &'static str = r#"
export interface DownloadEventHandler {
    downloadProgress(offset: BigInt, len: BigInt);
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "DownloadEventHandler")]
    pub type WebDownloadEventHandler;

    #[wasm_bindgen(catch, method, js_name = downloadProgress)]
    fn download_progress(
        this: &WebDownloadEventHandler,
        offset: u64,
        len: u64,
    ) -> Result<(), JsValue>;
}

#[wasm_bindgen(js_name = DownloaderClient)]
pub struct WebDownloaderClient {
    client: Rc<DownloaderClient>,
}

struct DownloadFile {
    file: JsDownloadableFile,
    pending_write: Option<PendingWrite>,
}

type PendingWrite = Pin<Box<dyn Future<Output = Result<(), js_sys::Error>>>>;

#[derive(Debug, thiserror::Error)]
#[error("download file write error: {message}")]
struct DownloadFileWriteError {
    pub message: String,
}

#[wasm_bindgen(js_class = DownloaderClient)]
impl WebDownloaderClient {
    #[wasm_bindgen(constructor)]
    pub fn from_download_url(
        download_url: &str,
        api_endpoint: web_sys::Url,
    ) -> Result<WebDownloaderClient, js_sys::Error> {
        let api_endpoint =
            url::Url::parse(&ToString::to_string(&api_endpoint.to_string())).unwrap();
        Ok(Self {
            client: Rc::new(DownloaderClient::from_download_url(
                download_url,
                api_endpoint,
                Transport::Both,
            )?),
        })
    }

    #[wasm_bindgen(js_name = fetchMeta)]
    pub async fn fetch_meta(&self) -> Result<DownloadMeta, js_sys::Error> {
        let meta = self.client.fetch_meta_async().await?;
        Ok(meta)
    }

    #[wasm_bindgen(js_name = download)]
    pub async fn download(
        &self,
        meta: &DownloadMeta,
        file: JsDownloadableFile,
        p2p_timeout_seconds: Option<f64>,
        event_handler: Option<WebDownloadEventHandler>,
    ) -> Result<(), js_sys::Error> {
        let p2p_timeout = p2p_timeout_seconds.map(Duration::from_secs_f64);
        let file_state = DownloadFile { file, pending_write: None };
        let decrypted_file = self.client.api_client.decrypt_file(meta, file_state);
        let download_progress = self
            .client
            .download_async(meta, decrypted_file, p2p_timeout);
        pin_mut!(download_progress);
        if let Some(event_handler) = &event_handler {
            while let Some(progress_state) = download_progress.try_next().await? {
                event_handler.download_progress(progress_state.current, progress_state.total)?;
            }
        } else {
            download_progress.await?;
        }
        Ok(())
    }
}

impl DownloadFile {
    fn poll_pending_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), DownloadFileWriteError>> {
        if let Some(pending_write) = &mut self.pending_write {
            ready!(pending_write.poll_unpin(cx)).map_err(DownloadFileWriteError::from)?;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for DownloadFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        ready!(self.as_mut().poll_pending_write(cx))?;
        let file: JsDownloadableFile = self.file.clone().into();
        let js_buf = buf.into();
        self.pending_write = Some(Box::pin(async move { file.write(js_buf).await }));
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        ready!(self.as_mut().poll_pending_write(cx))?;
        self.pending_write = None;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

impl From<js_sys::Error> for DownloadFileWriteError {
    fn from(from: js_sys::Error) -> Self {
        let message = from
            .dyn_ref::<JsString>()
            .map(|string| format!("{string}"))
            .unwrap_or_else(|| format!("{from:?}"));
        Self { message }
    }
}

impl From<DownloadFileWriteError> for io::Error {
    fn from(from: DownloadFileWriteError) -> Self {
        Self::new(io::ErrorKind::Other, from)
    }
}
