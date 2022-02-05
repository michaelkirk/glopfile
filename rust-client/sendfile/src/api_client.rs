use std::fs::File;
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::cipher::{CipherKey, ContentCipher};
use crate::websocket::{WebSocketConnection, WebSocketMessage};
use crate::{Error, Result};

// should this be configurable, or infinite even?
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

    pub fn provision_file(
        &self,
        file_name: String,
        file_size: u64,
    ) -> Result<ProvisionFileResponse> {
        let url = self.endpoint.join("/api/v1/files").expect("bad endpoint?");

        let file_meta = FileMeta { file_name, file_size };
        let metadata_json = serde_json::to_string(&file_meta)
            .map_err(|_| Error::InvalidInput("unserializable upload"))?;
        let encrypted_metadata = self.cipher().encrypt(metadata_json.as_bytes());

        let encoded_metadata = base64::encode(encrypted_metadata);
        debug!(
            "posting to url: {}, encrypted_metadata: {:?}",
            url, &encoded_metadata
        );

        let form = [("encrypted_metadata", encoded_metadata)];

        let response = self.http_client().post(url).form(&form).send()?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to provision file",
                status: response.status().as_u16(),
            });
        }

        let provision_file_response = response.json::<ProvisionFileResponse>()?;
        Ok(provision_file_response)
    }

    pub fn encrypt_file(&self, mut file: File) -> EncryptedFile {
        let mut plaintext = vec![];
        let _plaintext_len = file.read_to_end(&mut plaintext);
        // TODO - verify length matches that in metadata
        // assert_eq!(plaintext_len,

        // TODO stream
        let encrypted_bytes = ContentCipher::new(&self.cipher_key).encrypt(&plaintext);
        EncryptedFile { encrypted_bytes: encrypted_bytes.into() }
    }

    pub fn upload_file(&self, file: EncryptedFile, upload_path: &str) -> Result<()> {
        let url = self.endpoint.join(upload_path).expect("bad endpoint?");

        let response = self
            .http_client_builder()
            .timeout(CONTENT_TIMEOUT)
            .build()
            .expect("invalid timeout for http client?")
            .post(url)
            .body(file.encrypted_bytes)
            .send()?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to upload content",
                status: response.status().as_u16(),
            });
        }

        Ok(())
    }

    pub fn fetch_meta(&self, download_id: &DownloadId) -> Result<DownloadMeta> {
        let download_path = format!("/api/v1/download/{}", download_id);
        let url = self.endpoint.join(&download_path).expect("bad endpoint?");

        let response = self.http_client().get(url).send()?;

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
        let download_response = response.json::<EncodedDownloadMeta>()?;
        debug!("download_response: {:?}", download_response);
        let decoded_metadata: Vec<u8> = base64::decode(&download_response.meta)
            .map_err(|_| Error::InvalidInput("invalid base64 encoding of metadata"))?;
        let decrypted_metadata = self.cipher().decrypt(&decoded_metadata)?;
        let file_meta = FileMeta::try_from_encoded(&decrypted_metadata)?;
        Ok(DownloadMeta {
            encrypted_content_url: download_response.encrypted_content_url,
            file_meta,
        })
    }

    pub fn decrypted_file(
        &self,
        download_meta: &DownloadMeta,
        output_dir: Option<&Path>,
    ) -> DecryptedFile<'_> {
        DecryptedFile::new(self, download_meta, output_dir)
    }

    pub fn download_content(
        &self,
        download_meta: &DownloadMeta,
        mut decrypted_file: DecryptedFile,
    ) -> Result<()> {
        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;

        let response = self
            .http_client_builder()
            .timeout(CONTENT_TIMEOUT)
            .build()
            .expect("invalid timeout for http client?")
            .get(content_url)
            .send()?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to download content",
                status: response.status().as_u16(),
            });
        } else {
            debug!("fetched content successfully");
        }

        use io::Write;
        decrypted_file.write_all(&response.bytes()?)?;
        decrypted_file.flush()?;

        Ok(())
    }

    pub fn finish_download(&self, download_meta: &DownloadMeta) -> Result<()> {
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

        let response = request.send()?;
        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to finish download",
                status: response.status().as_u16(),
            });
        }

        Ok(())
    }

    pub fn connect_download_websocket(
        &self,
        encrypted_content_url: &str,
        handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<WebSocketConnection> {
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
            content_url.set_scheme("ws").expect("bad scheme?");
            content_url
        };
        let websocket = WebSocketConnection::connect(websocket_url, handle_incoming_message)?;
        Ok(websocket)
    }

    pub fn connect_upload_websocket(
        &self,
        upload_path: &str,
        handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<WebSocketConnection> {
        let websocket_url = {
            let mut upload_url = self
                .endpoint
                .join(upload_path)
                .map_err(|_| Error::InvalidInput("bad upload path"))?;
            upload_url
                .path_segments_mut()
                .map_err(|_| Error::InvalidInput("bad upload path"))?
                .push("ws");
            upload_url.set_scheme("ws").expect("bad scheme?");
            upload_url
        };
        let websocket = WebSocketConnection::connect(websocket_url, handle_incoming_message)?;
        Ok(websocket)
    }

    fn http_client_builder(&self) -> reqwest::blocking::ClientBuilder {
        reqwest::blocking::Client::builder()
    }

    fn http_client(&self) -> reqwest::blocking::Client {
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

#[derive(Debug)]
pub(crate) struct DownloadMeta {
    pub(crate) encrypted_content_url: String,
    pub(crate) file_meta: FileMeta,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct FileMeta {
    pub(crate) file_name: String,
    pub(crate) file_size: u64,
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
    output_path: PathBuf,
    file_size: u64,
    shared: Arc<Mutex<DecryptedFileShared>>,
}

struct DecryptedFileShared {
    data: Vec<u8>,
    completely_written: bool,
}

impl<'a> DecryptedFile<'a> {
    fn new(
        api_client: &'a ApiClient,
        download_meta: &DownloadMeta,
        output_dir: Option<&Path>,
    ) -> Self {
        let output_path: PathBuf = if let Some(output_dir) = output_dir {
            Path::join(output_dir, &download_meta.file_meta.file_name)
        } else {
            PathBuf::from(&download_meta.file_meta.file_name)
        };
        let data_len =
            usize::try_from(download_meta.file_meta.file_size).expect("file fits in memory");
        Self {
            api_client,
            output_path,
            file_size: download_meta.file_meta.file_size + ContentCipher::extra_ciphertext_len(),
            shared: Arc::new(Mutex::new(DecryptedFileShared {
                data: Vec::with_capacity(data_len),
                completely_written: false,
            })),
        }
    }
}

impl<'a> io::Write for DecryptedFile<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: stream rather
        let mut shared = self.shared.lock().unwrap();
        shared.data.extend(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let file_size = usize::try_from(self.file_size).expect("file fits in memory");

        let mut shared = self.shared.lock().unwrap();

        if shared.completely_written {
            return Ok(());
        }

        if shared.data.len() != file_size {
            // It isn't great to return success here and silently ignore the fact that we weren't able to flush
            // anything, but we should have streaming encryption soon and this won't happen then.
            return Ok(());
        }

        let plaintext = self
            .api_client
            .cipher()
            .decrypt(&shared.data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

        let file = File::create(&self.output_path)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(&plaintext)?;

        shared.completely_written = true;

        Ok(())
    }
}

impl<'a> Drop for DecryptedFile<'a> {
    fn drop(&mut self) {
        use io::Write;
        match self.flush() {
            Ok(()) => (),
            Err(error) => warn!("error writing decrypted file to disk: {error}"),
        }
    }
}
