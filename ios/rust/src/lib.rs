mod logger;

mod ffi {
    use crate::uploader::{FileUpload, FileUploadError, FileUploader, NewFileUploaderError};
    include!(concat!(env!("OUT_DIR"), "/lib.uniffi.rs"));
}

mod uploader;
