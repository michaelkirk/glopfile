use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::cipher::{CipherKey, ContentCipher};
use crate::{Error, Result};

pub(crate) struct ApiClient {
    cipher_key: CipherKey,
    endpoint: Url,
}

impl ApiClient {
    pub fn new(endpoint: Url, cipher_key: CipherKey) -> Self {
        Self {
            cipher_key,
            endpoint,
        }
    }

    fn cipher(&self) -> ContentCipher {
        ContentCipher::new(&self.cipher_key)
    }

    pub(crate) fn cipher_key(&self) -> &CipherKey {
        &self.cipher_key
    }

    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub fn provision_file(
        &self,
        file_name: String,
        file_size: u64,
    ) -> Result<ProvisionFileResponse> {
        let url = self.endpoint.join("/api/v1/files").expect("bad endpoint?");

        let file_meta = FileMeta {
            file_name,
            file_size,
        };
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

    pub fn upload_file(&self, mut file: File, upload_url: &str) -> Result<()> {
        let url = self.endpoint.join(upload_url).expect("bad endpoint?");

        let mut plaintext = vec![];
        let _plaintext_len = file.read_to_end(&mut plaintext);
        // TODO - verify length matches that in metadata
        // assert_eq!(plaintext_len,

        // TODO stream
        let encrypted_bytes = ContentCipher::new(&self.cipher_key).encrypt(&plaintext);

        // it might be a while before the downloader connects
        // TODO: make this configurable?
        let upload_timeout = std::time::Duration::from_secs(500);

        let response = self
            .http_client_builder()
            .timeout(upload_timeout)
            .build()
            .expect("invalid timeout for http client?")
            .post(url)
            .body(encrypted_bytes)
            .send()?;

        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to upload content",
                status: response.status().as_u16(),
            });
        }

        Ok(())
    }

    pub fn fetch_meta(&self, download_path: &str) -> Result<DownloadMeta> {
        let url = self.endpoint.join(download_path).expect("bad endpoint?");

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

    pub fn download_content(
        &self,
        download_meta: &DownloadMeta,
        output_dir: Option<&Path>,
    ) -> Result<()> {
        let content_url = self
            .endpoint
            .join(&download_meta.encrypted_content_url)
            .map_err(|_| Error::InvalidInput("bad content url"))?;

        let response = self.http_client().get(content_url).send()?;
        if !response.status().is_success() {
            return Err(Error::ClientHttpErrorResponse {
                message: "failed to download content",
                status: response.status().as_u16(),
            });
        } else {
            debug!("fetched content successfully");
        }

        use std::io::Write;

        // TODO: stream rather
        let mut ciphertext = vec![];
        {
            let mut ciphertext_writer = std::io::BufWriter::new(&mut ciphertext);
            ciphertext_writer.write_all(&response.bytes()?)?;
        }
        let plaintext = self.cipher().decrypt(&ciphertext)?;

        let output_path: PathBuf = if let Some(output_dir) = output_dir {
            Path::join(output_dir, &download_meta.file_meta.file_name)
        } else {
            PathBuf::from(&download_meta.file_meta.file_name)
        };

        let file = File::create(output_path)?;
        let mut writer = std::io::BufWriter::new(file);
        writer.write_all(&plaintext)?;

        Ok(())
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

#[derive(Debug, Deserialize)]
pub(crate) struct ProvisionFileResponse {
    pub(crate) upload_url: String,
    pub(crate) download_url: String,
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
