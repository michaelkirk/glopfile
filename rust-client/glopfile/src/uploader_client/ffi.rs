use std::sync::Arc;

use crate::{Error, NativeProvisionedFile, Result, Transport};

use super::UploaderClient;

impl UploaderClient {
    pub(crate) fn new_ffi(
        api_endpoint: &str,
        download_endpoint: &str,
        transport: Transport,
    ) -> Result<Self> {
        let api_endpoint = api_endpoint
            .parse()
            .map_err(|_| Error::InvalidInput("invalid api endpoint"))?;
        let download_endpoint = download_endpoint
            .parse()
            .map_err(|_| Error::InvalidInput("invalid api endpoint"))?;
        let client = Self::new(api_endpoint, download_endpoint, transport);
        Ok(client)
    }

    pub(crate) fn provision_file_ffi(&self, path: &str) -> Result<Arc<NativeProvisionedFile>> {
        let provisioned_file = self.provision_file(path.as_ref())?;
        Ok(provisioned_file.into())
    }

    pub(crate) fn upload_provisioned_file_ffi(
        &self,
        provisioned_file: &NativeProvisionedFile,
    ) -> Result<()> {
        self.upload_provisioned_file(provisioned_file.try_clone()?, drop)
    }
}
