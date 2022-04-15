use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::{pin_mut, Future, FutureExt};

#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

mod progress;

pub use progress::{Progress, ProgressState};

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use gloo_timers::future::{sleep as sleep_impl, TimeoutFuture as SleepImpl};
    } else {
        use tokio::time::{sleep as sleep_impl, Sleep as SleepImpl};
    }
}

#[pin_project::pin_project]
pub(crate) struct Sleep(#[pin] SleepImpl);

pub(crate) fn sleep(duration: Duration) -> Sleep {
    Sleep(sleep_impl(duration))
}

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().0.poll(cx)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Timed out")]
pub(crate) struct TimeoutError;

pub(crate) async fn timeout<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, TimeoutError> {
    let recv_future = future.fuse();
    let timeout_future = sleep(duration).fuse();
    pin_mut!(recv_future, timeout_future);
    futures::select! {
        recv_result = recv_future => Ok(recv_result),
        _ = timeout_future => Err(TimeoutError),
    }
}
