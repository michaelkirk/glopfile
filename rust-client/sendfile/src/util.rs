use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::future::Retry;
use backoff::Notify;
use futures::future::{abortable, AbortHandle, Aborted};
use futures::{pin_mut, Future, FutureExt};
use http::header;
use http::header::HeaderValue;

#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

mod progress;

pub use progress::{Progress, ProgressState};

use crate::Error;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use web::{send_sleep as send_sleep_impl, SendSleep, sleep as sleep_impl, Sleep as SleepImpl};
    } else {
        use tokio::time::{
            sleep as send_sleep_impl, sleep as sleep_impl, Sleep as SendSleep, Sleep as SleepImpl,
        };
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

pub(crate) type TimeoutResult<T> = Result<T, TimeoutError>;

pub(crate) async fn timeout<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, TimeoutError> {
    let future = future.fuse();
    let timeout_future = sleep(duration).fuse();
    pin_mut!(future, timeout_future);
    futures::select! {
        result = future => Ok(result),
        _ = timeout_future => Err(TimeoutError),
    }
}

pub(crate) fn abortable_timeout<'a, F: Future + 'a>(
    duration: Duration,
    future: F,
) -> (
    impl Future<Output = Result<F::Output, TimeoutError>> + 'a,
    AbortHandle,
) {
    let (timeout_future, abort_timeout_handle) = abortable(sleep(duration));
    let cancellable_timeout_future = async {
        let future = future.fuse();
        let timeout_future = timeout_future.fuse();
        pin_mut!(future, timeout_future);
        loop {
            futures::select! {
                result = future => break Ok(result),
                timeout_result = timeout_future => match timeout_result {
                    Ok(()) => break Err(TimeoutError),
                    Err(Aborted) => (),
                }
            }
        }
    };
    (cancellable_timeout_future, abort_timeout_handle)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NoopNotify;
impl<E> Notify<E> for NoopNotify {
    fn notify(&mut self, _error: E, _duration: Duration) {}
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Sleeper;

impl backoff::future::Sleeper for Sleeper {
    type Sleep = SendSleep;

    fn sleep(&self, duration: Duration) -> Self::Sleep {
        send_sleep_impl(duration)
    }
}

pub(crate) fn retry<I, E, Fn, Fut, B>(
    backoff: B,
    operation: Fn,
) -> Retry<Sleeper, B, NoopNotify, Fn, Fut>
where
    B: Backoff,
    Fn: FnMut() -> Fut,
    Fut: Future<Output = Result<I, backoff::Error<E>>>,
{
    retry_notify(backoff, operation, NoopNotify)
}

pub(crate) fn retry_notify<I, E, Fn, Fut, B, N>(
    mut backoff: B,
    operation: Fn,
    notify: N,
) -> Retry<Sleeper, B, N, Fn, Fut>
where
    B: Backoff,
    Fn: FnMut() -> Fut,
    Fut: Future<Output = Result<I, backoff::Error<E>>>,
    N: Notify<E>,
{
    backoff.reset();
    Retry::new(Sleeper, backoff, notify, operation)
}

pub(crate) trait ResponseExt: Sized {
    fn retry_after(&self) -> Result<Option<Duration>, Error>;
}

impl ResponseExt for reqwest::Response {
    fn retry_after(&self) -> Result<Option<Duration>, Error> {
        match self.headers().get(header::RETRY_AFTER) {
            Some(header) => parse_retry_after(header),
            None => Ok(None),
        }
    }
}

impl<T> ResponseExt for http::Response<T> {
    fn retry_after(&self) -> Result<Option<Duration>, Error> {
        match self.headers().get(header::RETRY_AFTER) {
            Some(header) => parse_retry_after(header),
            None => Ok(None),
        }
    }
}

pub(crate) fn parse_retry_after(header: &HeaderValue) -> Result<Option<Duration>, Error> {
    let retry_after = header
        .to_str()
        .map_err(|_| Error::InvalidServerResponse("Non-utf8 Retry-After header value"))?
        .parse()
        .map_err(|_| Error::InvalidServerResponse("Invalid Retry-After header value"))?;
    Ok(Some(Duration::from_secs(retry_after)))
}
