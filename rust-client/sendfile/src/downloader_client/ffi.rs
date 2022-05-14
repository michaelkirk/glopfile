use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::{DownloadMeta, Error, Result, Transport};

use super::DownloaderClient;

impl DownloaderClient {
    pub(crate) fn from_download_url_ffi(
        download_url: &str,
        api_endpoint: &str,
        transport: Transport,
    ) -> Result<Self> {
        let api_endpoint = api_endpoint
            .parse()
            .map_err(|_| Error::InvalidInput("invalid api endpoint"))?;
        let client = Self::from_download_url(download_url, api_endpoint, transport)?;
        Ok(client)
    }

    pub(crate) fn download_id_ffi(&self) -> String {
        self.download_id().to_string()
    }

    pub(crate) fn fetch_meta_ffi(&self) -> Result<Arc<DownloadMeta>> {
        let meta = self.fetch_meta()?;
        Ok(meta.into())
    }

    pub(crate) fn download_ffi(
        &self,
        meta: &DownloadMeta,
        output_dir: Option<String>,
        p2p_timeout: Option<Duration>,
    ) -> Result<()> {
        let output_dir = output_dir.map(PathBuf::from);
        self.download(meta, output_dir, p2p_timeout, drop)
    }
}
