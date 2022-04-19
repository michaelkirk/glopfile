use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::future::Retry;
use backoff::Notify;
use futures::Future;
use http::header;
use http::header::HeaderValue;

use crate::Error;

use super::timeout::{send_sleep, SendSleep};

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
        send_sleep(duration)
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
