use std::fs::File;
use std::path::Path;

use url::Url;

use crate::{ApiClient, CipherKey, Error, Result};

pub struct SenderClient {
    api_client: ApiClient,
}

impl SenderClient {
    pub fn new(endpoint: Url) -> Self {
        let api_client = ApiClient::new(endpoint, CipherKey::random());
        Self { api_client }
    }

    pub fn new_testing() -> Self {
        let endpoint = Url::parse("http://localhost:8080").expect("invalid hardcoded url");
        Self::new(endpoint)
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

        let download_url_without_cipher_key =
            self.download_url_without_cipher_key(&provision_response.download_url);

        let provisioned_file = ProvisionedFile {
            file,
            upload_url: provision_response.upload_url,
            download_url_without_cipher_key,
            // TODO: per file key?
            cipher_key: self.api_client.cipher_key().clone(),
        };

        Ok(provisioned_file)
    }

    fn download_url_without_cipher_key(&self, download_path: &str) -> Url {
        let mut url = self.api_client.endpoint().clone();
        url.set_path(download_path);
        url
    }

    pub fn upload_provisioned_file(&self, provisioned_file: ProvisionedFile) -> Result<()> {
        self.api_client
            .upload_file(provisioned_file.file, &provisioned_file.upload_url)
    }
}

#[derive(Debug)]
pub struct ProvisionedFile {
    file: File,
    upload_url: String,
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

    use std::path::PathBuf;

    use crate::init_test_logging;

    #[test]
    fn test_formatted_download_link() {
        let key = CipherKey::from_bytes([1u8; 32]);
        let p = ProvisionedFile {
            file: File::open("test_fixtures/sample_file.txt").unwrap(),
            upload_url: "/my/upload_url".to_string(),
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
