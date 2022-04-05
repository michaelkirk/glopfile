#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

mod progress;

pub use progress::{Progress, ProgressState};
