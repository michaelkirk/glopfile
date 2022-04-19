#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

mod progress;
mod retry;
mod timeout;

pub use progress::{Progress, ProgressState};
pub(crate) use retry::{retry, ResponseExt};
pub(crate) use timeout::{abortable_timeout, timeout, TimeoutError, TimeoutResult};
