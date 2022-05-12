#[cfg(target_arch = "wasm32")]
mod web;

use std::time::Duration;

use http::StatusCode;
use reqwest::Response;

use crate::util::{ResponseExt, TimeoutError};
use crate::websocket::{WebSocketCloseStatus, WebSocketError, WebSocketKnownCloseStatus};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("API status {status} - {message}")]
    ClientHttpErrorResponse {
        message: &'static str,
        status: u16,
        retry_after: Option<Duration>,
    },
    #[error("API error: {reason}")]
    ClientApiErrorResponse { reason: String },
    #[error("WebSocket closed with status {status}: {reason}")]
    WebSocketClosed {
        status: WebSocketCloseStatus,
        reason: String,
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

pub(crate) trait IntoRetriableResultExt: Sized {
    type Output;
    fn ok_or_retriable_err(
        self,
        message: &'static str,
    ) -> Result<Self::Output, backoff::Error<Error>>;
}

pub(crate) trait AsRetriableResultExt: Sized {
    type Output;
    fn as_retriable_result(self) -> Result<Self::Output, backoff::Error<Error>>;
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
            WebSocketError::Closed { status, reason } => Self::WebSocketClosed { status, reason },
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

impl<T: IntoResultExt> IntoRetriableResultExt for T {
    type Output = Self;
    fn ok_or_retriable_err(self, message: &'static str) -> Result<Self, backoff::Error<Error>> {
        self.ok_or(message).as_retriable_result()
    }
}

impl<T, E> AsRetriableResultExt for Result<T, E>
where
    Error: From<E>,
{
    type Output = T;
    fn as_retriable_result(self) -> Result<T, backoff::Error<Error>> {
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
            Error::WebSocketClosed { status: WebSocketCloseStatus::Known(status), .. } => {
                use WebSocketKnownCloseStatus::*;
                match status {
                    Normal
                    | Gone
                    | MissingProtocolExtension
                    | FrameTooLarge
                    | Forbidden
                    | InvalidFrameData
                    | MissingStatusCode
                    | UnsupportedFrameType
                    | ProtocolError => backoff::Error::permanent(error),
                    ConnectionClosed | InternalServerError | TlsError => {
                        backoff::Error::transient(error)
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
