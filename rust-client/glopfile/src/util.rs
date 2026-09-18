#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

mod progress;
mod retry;
mod spawn;
mod timeout;

pub use progress::{Progress, ProgressState};
pub(crate) use retry::{retry, ResponseExt};
pub(crate) use spawn::spawn_local;
pub(crate) use timeout::{TimeoutError, TimeoutExt, TimeoutResult};
