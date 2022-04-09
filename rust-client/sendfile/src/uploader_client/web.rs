use std::pin::Pin;
use std::rc::Rc;
use std::task;
use std::task::Poll;
use std::{io, mem};

use futures::{pin_mut, ready, AsyncRead, FutureExt, TryStreamExt};
use js_sys::{JsString, Promise, Uint8Array};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

use crate::transport::Transport;
use crate::util::web::return_promise;
use crate::ProvisionedFile;

use super::{UploadableFile, UploaderClient};

#[wasm_bindgen(typescript_custom_section)]
const UPLOADABLE_FILE_INTERFACE: &'static str = r#"
export interface UploadableFile {
    len(): BigInt;
    readAt(offset: BigInt, len: BigInt): Promise<Uint8Array>;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "UploadableFile")]
    pub type WebUploadableFile;

    #[wasm_bindgen(method)]
    fn len(this: &WebUploadableFile) -> u64;

    #[wasm_bindgen(method, js_name = readAt)]
    fn read_at(this: &WebUploadableFile, offset: u64, len: u64) -> Promise;
}

#[wasm_bindgen(typescript_custom_section)]
const UPLOAD_EVENT_HANDLER_INTERFACE: &'static str = r#"
export interface UploadEventHandler {
    uploadProgress(offset: BigInt, len: BigInt);
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "UploadEventHandler")]
    pub type WebUploadEventHandler;

    #[wasm_bindgen(catch, method, js_name = uploadProgress)]
    fn upload_progress(this: &WebUploadEventHandler, offset: u64, len: u64) -> Result<(), JsValue>;
}

#[wasm_bindgen(js_name = ProvisionedFile)]
pub struct WebProvisionedFile {
    inner: Option<ProvisionedFile<UploadFile>>,
}

#[wasm_bindgen(js_name = UploaderClient)]
pub struct WebUploaderClient {
    client: Rc<UploaderClient>,
}

struct UploadFile {
    file: WebUploadableFile,
    offset: u64,
    pending_read: UploadFileReadState,
}

enum UploadFileReadState {
    Idle,
    Reading { pending_read: JsFuture },
    Available { data: Uint8Array },
}

#[derive(Debug, thiserror::Error)]
#[error("upload file read error: {message}")]
struct UploadFileReadError {
    pub message: String,
}

#[wasm_bindgen(js_class = UploaderClient)]
impl WebUploaderClient {
    #[wasm_bindgen(constructor)]
    pub fn new(
        api_endpoint: web_sys::Url,
        download_endpoint: web_sys::Url,
    ) -> Result<WebUploaderClient, js_sys::Error> {
        let api_endpoint =
            url::Url::parse(&ToString::to_string(&api_endpoint.to_string())).unwrap();
        let download_endpoint =
            url::Url::parse(&ToString::to_string(&download_endpoint.to_string())).unwrap();
        Ok(Self {
            client: Rc::new(UploaderClient::new(
                api_endpoint,
                download_endpoint,
                Transport::Both,
            )),
        })
    }

    #[wasm_bindgen(js_name = provisionFile)]
    pub fn provision_file(&self, file: WebUploadableFile, file_name: String) -> Promise {
        let file_state = UploadFile { file, offset: 0, pending_read: Default::default() };
        let client = Rc::clone(&self.client);
        return_promise(async move {
            let provisioned_file = client.provision_file_async(file_state, file_name).await?;
            Ok(WebProvisionedFile { inner: Some(provisioned_file) })
        })
    }

    #[wasm_bindgen(js_name = uploadFile)]
    pub fn upload_file(
        self,
        provisioned_file: &mut WebProvisionedFile,
        event_handler: Option<WebUploadEventHandler>,
    ) -> Promise {
        let client = Rc::clone(&self.client);
        let provisioned_file = provisioned_file
            .inner
            .take()
            .expect("ProvisionedFile used after being consumed");
        return_promise(async move {
            let upload_progress = client.upload_provisioned_file_async(provisioned_file);
            pin_mut!(upload_progress);
            while let Some(progress_state) = upload_progress.try_next().await? {
                if let Some(event_handler) = &event_handler {
                    event_handler.upload_progress(progress_state.current, progress_state.total)?;
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }
}

#[wasm_bindgen(js_class = ProvisionedFile)]
impl WebProvisionedFile {
    #[wasm_bindgen(js_name = downloadURLWithCipherKey)]
    pub fn download_url_with_cipher_key(&self) -> Result<web_sys::Url, js_sys::Error> {
        let inner = self
            .inner
            .as_ref()
            .expect("ProvisionedFile used after being consumed");
        let url_string = inner.formatted_download_url_and_key();
        let url = web_sys::Url::new(&url_string)?;
        Ok(url)
    }
}

#[async_trait::async_trait(?Send)]
impl UploadableFile for UploadFile {
    async fn len(&self) -> io::Result<u64> {
        Ok(self.file.len())
    }
}

impl AsyncRead for UploadFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let data = match mem::take(&mut self.pending_read) {
            pending_read @ (UploadFileReadState::Reading { .. } | UploadFileReadState::Idle) => {
                self.pending_read = pending_read;
                let pending_read = match &mut self.pending_read {
                    UploadFileReadState::Reading { pending_read } => pending_read,
                    UploadFileReadState::Idle => {
                        let read_promise = self
                            .file
                            .read_at(self.offset, buf.len().try_into().unwrap());
                        self.pending_read.insert(read_promise.into())
                    }
                    UploadFileReadState::Available { .. } => unreachable!(),
                };
                let data = ready!(pending_read.poll_unpin(cx))
                    .map_err(UploadFileReadError::from)?
                    .dyn_into::<Uint8Array>()
                    .expect("UploadableFile.read_at returns a Uint8Array");
                self.pending_read = UploadFileReadState::Idle;
                self.offset += u64::from(data.length());
                data
            }
            UploadFileReadState::Available { data } => data,
        };

        let data_len = usize::try_from(data.length()).expect("usize is at least 32 bits");
        let read_len = data_len.min(buf.len());
        debug!("returning {read_len} of {data_len} bytes");
        data.subarray(0, read_len as u32)
            .copy_to(&mut buf[..read_len]);
        if data_len > buf.len() {
            self.pending_read = UploadFileReadState::Available {
                data: data.subarray(buf.len() as u32, data.length()),
            };
        }
        Poll::Ready(Ok(read_len))
    }
}

impl Default for UploadFileReadState {
    fn default() -> Self {
        Self::Idle
    }
}

impl UploadFileReadState {
    fn insert(&mut self, pending_read: JsFuture) -> &mut JsFuture {
        *self = Self::Reading { pending_read };
        match self {
            Self::Reading { pending_read } => pending_read,
            _ => unreachable!(),
        }
    }
}

impl From<JsValue> for UploadFileReadError {
    fn from(from: JsValue) -> Self {
        let message = from
            .dyn_ref::<JsString>()
            .map(|string| format!("{string}"))
            .unwrap_or_else(|| format!("{from:?}"));
        Self { message }
    }
}

impl From<UploadFileReadError> for io::Error {
    fn from(from: UploadFileReadError) -> Self {
        Self::new(io::ErrorKind::Other, from)
    }
}
