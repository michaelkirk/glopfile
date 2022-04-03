use std::io;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use futures::{ready, AsyncWrite, FutureExt};
use instant::Duration;
use js_sys::{JsString, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::DownloaderClient;
use crate::api_client::DownloadMeta;
use crate::transport::Transport;
use crate::util::web::return_promise;

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

    #[wasm_bindgen(method)]
    fn write(this: &JsDownloadableFile, data: Uint8Array) -> Promise;
}

#[wasm_bindgen(js_name = DownloaderClient)]
pub struct WebDownloaderClient {
    client: Rc<DownloaderClient>,
}

struct DownloadFile {
    file: JsDownloadableFile,
    pending_write: Option<JsFuture>,
}

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
    pub fn fetch_meta(&self) -> Promise {
        let client = Rc::clone(&self.client);
        return_promise(async move {
            let meta = client.fetch_meta_async().await?;
            Ok(meta)
        })
    }

    #[wasm_bindgen(js_name = download)]
    pub fn download(
        &self,
        meta: &DownloadMeta,
        file: JsDownloadableFile,
        p2p_timeout_seconds: Option<f64>,
    ) -> Promise {
        let meta = meta.clone();
        let p2p_timeout = p2p_timeout_seconds.map(Duration::from_secs_f64);
        let file_state = DownloadFile { file, pending_write: None };
        let client = Rc::clone(&self.client);
        return_promise(async move {
            let decrypted_file = client.api_client.decrypt_file(&meta, file_state);
            client
                .download_async(&meta, decrypted_file, p2p_timeout)
                .await?;
            Ok(JsValue::UNDEFINED)
        })
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
        let write_promise = self.file.write(buf.into());
        self.pending_write = Some(write_promise.into());
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

impl From<JsValue> for DownloadFileWriteError {
    fn from(from: JsValue) -> Self {
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
