#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use url::Url;

use crate::{ApiClient, CipherKey, Error, Result};

pub struct ReceiverClient {
    api_client: ApiClient,
    download_path: String,
    output_dir: Option<PathBuf>,
}

impl ReceiverClient {
    pub fn from_download_url(download_url_str: &str) -> Result<Self> {
        let (endpoint, download_path, cipher_key) = Self::parse_download_url(download_url_str)?;

        let api_client = ApiClient::new(endpoint, cipher_key);
        Ok(Self {
            api_client,
            download_path,
            output_dir: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn set_output_dir(&mut self, path: &Path) {
        self.output_dir = Some(path.into());
    }

    pub fn download(&self) -> Result<()> {
        let meta = self.api_client.fetch_meta(&self.download_path)?;
        self.api_client
            .download_content(&meta, self.output_dir.as_deref())
    }

    fn parse_download_url(download_url_str: &str) -> Result<(Url, String, CipherKey)> {
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

        let endpoint = {
            // this seems dumb... I'd like to do something like `download_url.origin().as_url()`
            let mut u = download_url.clone();
            u.set_path("");
            u.set_fragment(None);
            u
        };

        let download_path = download_url.path().to_string();
        if download_path.is_empty() {
            // TODO better validation
            return Err(Error::InvalidInput("invalid download path"));
        }

        Ok((endpoint, download_path, cipher_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_download_url() {
        let download_url =
            "https://foo.bar:123/their/download#cipher_key=AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

        let (endpoint, download_path, key) =
            ReceiverClient::parse_download_url(download_url).unwrap();

        assert_eq!(endpoint, Url::parse("https://foo.bar:123").unwrap());
        assert_eq!(download_path, "/their/download");
        assert_eq!(key.bytes(), &[1u8; 32]);
    }
}
