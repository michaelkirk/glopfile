use std::fs::File;
use std::path::Path;

use url::Url;

use crate::{ApiClient, CipherKey, DownloadId, Error, Result};

pub struct SenderClient {
    api_client: ApiClient,
    download_endpoint: Url,
}

impl SenderClient {
    pub fn new(api_endpoint: Url, download_endpoint: Url) -> Self {
        let api_client = ApiClient::new(api_endpoint, CipherKey::random());
        Self {
            api_client,
            download_endpoint,
        }
    }

    pub fn new_testing() -> Self {
        let api_endpoint = Url::parse("http://localhost:8080").expect("invalid hardcoded url");
        let download_endpoint = Url::parse("http://localhost:3000").expect("invalid hardcoded url");
        Self::new(api_endpoint, download_endpoint)
    }

    pub fn provision_file(&self, path: &Path) -> Result<ProvisionedFile> {
        let file = File::open(path)?;

        let file_size = file.metadata()?.len();
        let file_name = path
            .file_name()
            .ok_or(Error::InvalidInput("invalid file path"))?;

        let provision_response = self
            .api_client
            .provision_file(file_name.to_string_lossy().to_string(), file_size)?;

        let download_id_str = provision_response
            .download_url
            .strip_prefix("/api/v1/download/")
            .ok_or_else(|| {
                error!(
                    "api response was missing the expected download prefix. download_url: {}",
                    provision_response.download_url
                );
                Error::InvalidServerResponse(
                    "api response was missing the expected download prefix.",
                )
            })?;

        let download_id = DownloadId::new(download_id_str.to_string());

        let download_url_without_cipher_key = self.download_url_without_cipher_key(&download_id);

        // If the server ever changes to return non-relative URL's we'll have to change this logic
        // to try both relative and absolute URLs
        let upload_path = provision_response.upload_url;
        let provisioned_file = ProvisionedFile {
            file,
            upload_path,
            download_url_without_cipher_key,
            // TODO: per file key?
            cipher_key: self.api_client.cipher_key().clone(),
        };

        Ok(provisioned_file)
    }

    fn download_url_without_cipher_key(&self, download_id: &DownloadId) -> Url {
        let mut url = self.download_endpoint.clone();
        url.set_path(&format!("/download/{}", download_id));
        url
    }

    pub fn upload_provisioned_file(&self, provisioned_file: ProvisionedFile) -> Result<()> {
        self.api_client
            .upload_file(provisioned_file.file, &provisioned_file.upload_path)
    }
}

#[derive(Debug)]
pub struct ProvisionedFile {
    file: File,
    upload_path: String,
    download_url_without_cipher_key: Url,
    cipher_key: CipherKey,
}

impl ProvisionedFile {
    /// The download link to get the uploaded file.
    pub fn formatted_download_url_and_key(&self) -> String {
        format!(
            "{}#cipher_key={}",
            self.download_url_without_cipher_key,
            self.cipher_key.serialized()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_logging;

    #[test]
    fn test_download_url() {
        init_test_logging();

        let sender_client = SenderClient::new_testing();
        let download_id = DownloadId::new("abc123".to_string());
        let url = sender_client.download_url_without_cipher_key(&download_id);
        assert_eq!(
            url,
            Url::parse("http://localhost:3000/download/abc123").unwrap()
        );
    }

    mod provisioned_file_tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn test_formatted_download_link() {
            init_test_logging();

            let key = CipherKey::from_bytes([1u8; 32]);
            let p = ProvisionedFile {
                file: File::open("test_fixtures/sample_file.txt").unwrap(),
                upload_path: "/path/to/upload/123".to_string(),
                download_url_without_cipher_key: Url::parse(
                    "https://123.invalid:1234/their/download/456",
                )
                .unwrap(),
                cipher_key: key,
            };
            assert_eq!(
                p.formatted_download_url_and_key(),
                "https://123.invalid:1234/their/download/456#cipher_key=AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
            )
        }

        #[test]
        fn send_non_existent_file() {
            init_test_logging();

            let sender = SenderClient::new_testing();
            let path = PathBuf::from("path/to/non-existent-file");
            assert!(matches!(
                sender.provision_file(&path),
                Err(Error::IO { .. }),
            ));
        }
    }
}
