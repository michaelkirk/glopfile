#[cfg(target_arch = "wasm32")]
mod web;

use std::time::Duration;

use http::StatusCode;
use reqwest::Response;

use crate::util::{ResponseExt, TimeoutError};
use crate::websocket::WebSocketError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("API status {status} - {message}")]
    ClientHttpErrorResponse {
        message: &'static str,
        status: u16,
        retry_after: Option<Duration>,
    },
    #[error("IO Error: {source}")]
    IO {
        #[from]
        source: std::io::Error,
    },
    #[error("HTTP Client error: {source}")]
    HTTPClient {
        #[from]
        source: reqwest::Error,
    },
    #[error("RTC Data Channel error: {source}")]
    RTCDataChannel {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("WebSocket Client error: {source}")]
    WebSocketClient {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("Invalid message from peer: {source}")]
    InvalidPeerMessage {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("Invalid cipher key")]
    InvalidCipherKey,
    #[error("Decryption error")]
    Decrypt,
    #[error("Invalid server response: {0}")]
    InvalidServerResponse(&'static str),
    #[error("Timed out")]
    Timeout,
}

pub(crate) trait IntoResultExt: Sized {
    fn ok_or(self, message: &'static str) -> Result<Self, Error>;
}

pub(crate) trait IntoBackoffResultExt: Sized {
    type Output;
    fn ok_or_backoff(self, message: &'static str) -> Result<Self::Output, backoff::Error<Error>>;
}

pub(crate) trait BackoffResultExt: Sized {
    type Output;
    fn backoff(self) -> Result<Self::Output, backoff::Error<Error>>;
}

impl Error {
    pub(crate) fn rtc_err<E>(error: E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::RTCDataChannel { source: error.into() }
    }
}

impl From<TimeoutError> for Error {
    fn from(TimeoutError {}: TimeoutError) -> Self {
        Self::Timeout
    }
}

impl From<WebSocketError> for Error {
    fn from(error: WebSocketError) -> Self {
        match error {
            WebSocketError::ClientHttpErrorResponse { message, status, retry_after } => {
                Self::ClientHttpErrorResponse { message, status, retry_after }
            }
            source @ WebSocketError::Closed => Self::WebSocketClient { source: Box::new(source) },
            WebSocketError::IO { source } => Self::IO { source },
            WebSocketError::WebSocketClient { source } => Self::WebSocketClient { source },
            WebSocketError::InvalidMessage { source } => {
                Self::InvalidPeerMessage { source: Box::new(source) }
            }
        }
    }
}

impl IntoResultExt for Response {
    fn ok_or(self, message: &'static str) -> Result<Response, Error> {
        match self.status() {
            status if status.is_success() => Ok(self),
            status => Err(Error::ClientHttpErrorResponse {
                message,
                status: status.as_u16(),
                retry_after: self.retry_after()?,
            }),
        }
    }
}

impl<T: IntoResultExt> IntoBackoffResultExt for T {
    type Output = Self;
    fn ok_or_backoff(self, message: &'static str) -> Result<Self, backoff::Error<Error>> {
        self.ok_or(message).backoff()
    }
}

impl<T, E> BackoffResultExt for Result<T, E>
where
    Error: From<E>,
{
    type Output = T;
    fn backoff(self) -> Result<T, backoff::Error<Error>> {
        self.map_err(Error::from).map_err(|error| match &error {
            Error::ClientHttpErrorResponse { retry_after: Some(retry_after), .. } => {
                let retry_after = retry_after.clone();
                backoff::Error::retry_after(error, retry_after)
            }
            Error::ClientHttpErrorResponse { status, retry_after: None, .. } => {
                match StatusCode::from_u16(*status) {
                    Ok(status) if status.is_server_error() => backoff::Error::transient(error),
                    Ok(_status) => backoff::Error::permanent(error),
                    Err(status_error) => {
                        warn!(
                            "invalid HTTP status code {status} from server in response {error}: \
                             {status_error}"
                        );
                        backoff::Error::permanent(error)
                    }
                }
            }
            Error::HTTPClient { .. } | Error::IO { .. } | Error::Timeout { .. } => {
                backoff::Error::transient(error)
            }
            _ => backoff::Error::permanent(error),
        })
    }
}
