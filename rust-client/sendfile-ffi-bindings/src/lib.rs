use std::fs;
use std::path::Path;

use serde::Deserialize;
use uniffi_bindgen::bindings::TargetLanguage;
use uniffi_bindgen::interface::ComponentInterface;
use uniffi_bindgen::MergeWith;

pub use uniffi_bindgen::bindings;

#[derive(Debug, Clone, Default, Deserialize)]
struct Config {
    #[serde(default)]
    bindings: bindings::Config,
}

pub fn write_bindings(
    config_path: Option<impl AsRef<Path>>,
    out_dir: impl AsRef<Path>,
    language: TargetLanguage,
    try_format_code: bool,
) -> anyhow::Result<()> {
    let ci: ComponentInterface = include_str!("../../sendfile/src/lib.udl").parse()?;
    let default_config = bindings::Config::from(&ci);
    let config = match config_path {
        Some(config_path) => toml::de::from_str::<Config>(&fs::read_to_string(config_path)?)?
            .bindings
            .merge_with(&default_config),
        None => default_config,
    };
    uniffi_bindgen::bindings::write_bindings(&config, &ci, out_dir, language, try_format_code)
}
