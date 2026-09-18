use std::path::Path;

use uniffi_bindgen::bindings::{generate, GenerateOptions, TargetLanguage};

pub use uniffi_bindgen::bindings;

/// The glopfile UDL, resolved at compile time so callers don't need the source tree.
const GLOPFILE_UDL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../glopfile/src/lib.udl");

pub fn write_bindings(
    config_path: Option<impl AsRef<Path>>,
    out_dir: impl AsRef<Path>,
    language: TargetLanguage,
    try_format_code: bool,
) -> anyhow::Result<()> {
    generate(GenerateOptions {
        languages: vec![language],
        source: GLOPFILE_UDL.into(),
        out_dir: path_arg(out_dir.as_ref())?,
        config_override: config_path
            .map(|path| path_arg(path.as_ref()))
            .transpose()?,
        format: try_format_code,
        ..GenerateOptions::default()
    })
}

fn path_arg(path: &Path) -> anyhow::Result<camino::Utf8PathBuf> {
    camino::Utf8PathBuf::from_path_buf(path.to_path_buf())
        .map_err(|path| anyhow::anyhow!("path is not valid utf-8: {}", path.display()))
}
