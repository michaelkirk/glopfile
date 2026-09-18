use std::error::Error;

use sendfile_ffi_bindings::bindings::TargetLanguage;

const OUT_DIR: &str = "../Sendfile/src/FFI/Rust/SendfileRustFFI";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={OUT_DIR}");
    println!("cargo:rerun-if-changed=uniffi.toml");
    println!("cargo:rerun-if-changed=uniffi.sendfile.toml");

    uniffi_build::generate_scaffolding("src/lib.udl")?;

    uniffi_bindgen::generate_bindings(
        "src/lib.udl",
        Some("uniffi.toml"),
        vec!["swift"],
        Some(OUT_DIR),
        true,
    )?;

    sendfile_ffi_bindings::write_bindings(
        Some("uniffi.sendfile.toml"),
        OUT_DIR,
        TargetLanguage::Swift,
        true,
    )?;

    Ok(())
}
