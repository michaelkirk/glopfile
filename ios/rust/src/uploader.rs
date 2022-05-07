use std::{path::Path, sync::Arc};

use crossbeam_utils::atomic::AtomicCell;
use sendfile::{UploaderClient, Transport};
use url::Url;

use crate::logger;

pub struct FileUploader {
    client: Arc<UploaderClient>,
}

pub struct FileUpload {
    upload: Box<dyn Fn() -> Result<(), FileUploadError> + Send + Sync + 'static>,
    url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum NewFileUploaderError {
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(#[from] url::ParseError),
}

#[derive(Debug, thiserror::Error)]
pub enum FileUploadError {
    #[error("sendfile error: {0}")]
    Sendfile(#[from] sendfile::Error),
}

impl FileUploader {
    pub fn new(
        api_endpoint: &str,
        download_endpoint: &str,
    ) -> Result<FileUploader, NewFileUploaderError> {
        logger::set_logger();
        let api_endpoint = Url::parse(api_endpoint)?;
        let download_endpoint = Url::parse(download_endpoint)?;
        let client = Arc::new(UploaderClient::new(api_endpoint, download_endpoint, Transport::Both));
        Ok(Self { client })
    }

    pub fn provision_file(&self, path: &str) -> Result<Arc<FileUpload>, FileUploadError> {
        // HACK provisioned_file is an unnamable type and can't be stored in a struct. Store it in a closure instead.
        let provisioned_file = self
            .client
            .provision_file(Path::new(path))
            .map_err(|error| {
                log::warn!("error provisioning file: {error}", error = error);
                error
            })?;
        let url = provisioned_file.formatted_download_url_and_key();
        let provisioned_file = AtomicCell::new(Some(provisioned_file));
        let client = Arc::clone(&self.client);
        Ok(Arc::new(FileUpload {
            url,
            upload: Box::new(move || {
                if let Some(provisioned_file) = provisioned_file.take() {
                    client.upload_provisioned_file(provisioned_file, |_| {})?;
                }
                Ok(())
            }),
        }))
    }
}

impl FileUpload {
    pub fn url(&self) -> String {
        self.url.to_string()
    }

    pub fn upload(&self) -> Result<(), FileUploadError> {
        (self.upload)()
    }
}
