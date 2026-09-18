use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::future::{abortable, AbortHandle, Abortable, Aborted, Fuse};
use futures::{ready, Future, FutureExt};

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

pub(crate) trait TimeoutExt: Future + Sized {
    fn timeout(self, duration: Duration) -> TimeoutFuture<Self> {
        TimeoutFuture { future: self, timeout: sleep(duration) }
    }
    fn abortable_timeout(self, duration: Duration) -> AbortableTimeoutFuture<Self> {
        let (timeout, handle) = abortable(sleep(duration));
        let timeout = timeout.fuse();
        AbortableTimeoutFuture { future: self, timeout, handle }
    }
}

impl<T: Future + Sized> TimeoutExt for T {}
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Timed out")]
pub(crate) struct TimeoutError;

pub(crate) type TimeoutResult<T> = Result<T, TimeoutError>;

#[pin_project::pin_project]
pub(crate) struct TimeoutFuture<F: Future> {
    #[pin]
    future: F,
    #[pin]
    timeout: Sleep,
}

#[pin_project::pin_project]
pub(crate) struct AbortableTimeoutFuture<F: Future> {
    #[pin]
    future: F,
    #[pin]
    timeout: Fuse<Abortable<Sleep>>,
    handle: AbortHandle,
}

//
// TimeoutFuture impls
//

impl<F: Future> Future for TimeoutFuture<F> {
    type Output = TimeoutResult<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let projected = self.project();
        if let Poll::Ready(output) = projected.future.poll(cx) {
            return Poll::Ready(Ok(output));
        }
        let () = ready!(projected.timeout.poll(cx));
        Poll::Ready(Err(TimeoutError))
    }
}

//
// AbortableTimeoutFuture impls
//

impl<F: Future> AbortableTimeoutFuture<F> {
    pub(crate) fn timeout_handle(&self) -> &AbortHandle {
        &self.handle
    }
}

impl<F: Future> Future for AbortableTimeoutFuture<F> {
    type Output = TimeoutResult<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let projected = self.project();
        if let Poll::Ready(output) = projected.future.poll(cx) {
            return Poll::Ready(Ok(output));
        }
        match ready!(projected.timeout.poll(cx)) {
            Ok(()) => Poll::Ready(Err(TimeoutError)),
            Err(Aborted) => Poll::Pending,
        }
    }
}

//
// Sleep impls
//

impl Future for Sleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().0.poll(cx)
    }
}
