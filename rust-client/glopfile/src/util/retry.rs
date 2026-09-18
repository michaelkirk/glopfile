use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};
use futures::Future;
use http::header;
use http::header::HeaderValue;

use crate::error::{Retriability, RetryError};
use crate::Error;

/// 500ms, growing by half each time, jittered, never waiting more than a minute
/// at once nor retrying for more than a quarter of an hour.
fn schedule() -> ExponentialBuilder {
    ExponentialBuilder::new()
        .with_min_delay(Duration::from_millis(500))
        .with_factor(1.5)
        .with_jitter()
        .with_max_delay(Duration::from_secs(60))
        .without_max_times()
        .with_total_delay(Some(Duration::from_secs(900)))
}

/// Retries `operation` until it succeeds, fails permanently, or runs out of time.
pub(crate) async fn retry<T, Fut, OperationFn>(operation: OperationFn) -> Result<T, Error>
where
    OperationFn: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RetryError>>,
{
    operation
        .retry(schedule())
        .when(|error: &RetryError| error.retriability() != Retriability::Permanent)
        .adjust(|error: &RetryError, scheduled| match error.retriability() {
            Retriability::After(retry_after) => Some(retry_after),
            _ => scheduled,
        })
        .await
        .map_err(Error::from)
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
