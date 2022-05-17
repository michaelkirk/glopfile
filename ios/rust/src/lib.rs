mod logger;

pub mod ffi {
    use crate::logger::set_logger as set_rust_logger;
    include!(concat!(env!("OUT_DIR"), "/lib.uniffi.rs"));
}

pub use sendfile::ffi::*;
