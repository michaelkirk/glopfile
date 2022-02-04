#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use url::Url;

use crate::{ApiClient, CipherKey, DownloadId, Error, Result};

pub struct ReceiverClient {
    api_client: ApiClient,
    download_id: DownloadId,
    output_dir: Option<PathBuf>,
}

impl ReceiverClient {
    pub fn from_download_url(download_url_str: &str, api_endpoint: Url) -> Result<Self> {
        let (download_id, cipher_key) = Self::parse_download_url(download_url_str)?;

        let api_client = ApiClient::new(api_endpoint, cipher_key);
        Ok(Self {
            api_client,
            download_id,
            output_dir: None,
        })
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

    pub fn download(&self) -> Result<()> {
        let meta = self.api_client.fetch_meta(&self.download_id)?;
        self.api_client
            .download_content(&meta, self.output_dir.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_download_url() {
        let download_url =
            "https://foo.bar:123/download/123abc#cipher_key=AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

        let (download_id, key) = ReceiverClient::parse_download_url(download_url).unwrap();

        assert_eq!(download_id, DownloadId::new("123abc".to_string()));
        assert_eq!(key.bytes(), &[1u8; 32]);
    }
}
