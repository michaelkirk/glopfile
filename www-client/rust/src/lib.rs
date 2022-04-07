use wasm_bindgen::prelude::*;

#[macro_use]
extern crate log;

pub use sendfile::{UploadableFile, UploaderClient};

#[wasm_bindgen(start)]
pub fn init() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Debug).unwrap();
    debug!("{} initialized", env!("CARGO_PKG_NAME"));
    Ok(())
}
