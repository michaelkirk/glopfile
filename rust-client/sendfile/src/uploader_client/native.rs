use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::AsyncRead;
use tokio::fs::File;
use tokio_util::compat::TokioAsyncReadCompatExt;

use super::{ProvisionedFile, UploadableFile, UploaderClient};
use crate::{Error, Result};

pub struct NativeUploadFile {
    file: tokio_util::compat::Compat<File>,
}

pub type NativeProvisionedFile = ProvisionedFile<NativeUploadFile>;

impl UploaderClient {
    pub fn provision_file(&self, path: &std::path::Path) -> Result<NativeProvisionedFile> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(async {
                let file = tokio::fs::File::open(path).await?;
                let file_name = path
                    .file_name()
                    .ok_or(Error::InvalidInput("invalid file path"))?
                    .to_string_lossy()
                    .to_string();

                let file = NativeUploadFile { file: file.compat() };
                self.provision_file_async(file, file_name).await
            })
    }

    pub fn upload_provisioned_file(&self, provisioned_file: NativeProvisionedFile) -> Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(self.upload_provisioned_file_async(provisioned_file))
    }
}

impl AsyncRead for NativeUploadFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.file).poll_read(cx, buf)
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [io::IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.file).poll_read_vectored(cx, bufs)
    }
}

#[async_trait::async_trait(?Send)]
impl UploadableFile for NativeUploadFile {
    async fn len(&self) -> io::Result<u64> {
        Ok(self.file.get_ref().metadata().await?.len())
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::*;
    use crate::init_test_logging;

    mod provisioned_file_tests {
        use super::*;

        #[test]
        fn upload_non_existent_file() {
            init_test_logging();

            let uploader = UploaderClient::new_testing();
            let path = PathBuf::from("path/to/non-existent-file");
            assert!(matches!(
                uploader.provision_file(&path),
                Err(Error::IO { .. }),
            ));
        }
    }
}
