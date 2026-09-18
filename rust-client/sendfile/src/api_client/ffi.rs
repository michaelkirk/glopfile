use super::{DownloadMeta, FileMeta};

impl DownloadMeta {
    pub(crate) fn file_meta_ffi(&self) -> FileMeta {
        self.file_meta.clone()
    }
}
