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
    #[error("API error: {reason} - {message}")]
    ClientApiErrorResponse {
        message: &'static str,
        reason: String,
    },
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
    type Output;
    fn ok_or(self, message: &'static str) -> Result<Self::Output, Error>;
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
    // TODO We should probably rename to IntoRetriableResultExt, but that already exists...
    #[allow(clippy::wrong_self_convention)]
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
    type Output = Self;
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
    type Output = T::Output;
    fn ok_or_retriable_err(
        self,
        message: &'static str,
    ) -> Result<Self::Output, backoff::Error<Error>> {
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
                let retry_after = *retry_after;
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
            Error::HTTPClient { .. } | Error::IO { .. } | Error::Timeout => {
                backoff::Error::transient(error)
            }
            _ => backoff::Error::permanent(error),
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io;
    use std::num::NonZeroU16;

    use super::*;

    /// How the retry loop should treat an error, flattened for comparison.
    #[derive(Debug, PartialEq, Eq)]
    enum Classified {
        Permanent,
        Transient,
        After(Duration),
    }

    fn classify(error: Error) -> Classified {
        match Result::<(), Error>::Err(error).as_retriable_result() {
            Err(backoff::Error::Permanent(_)) => Classified::Permanent,
            Err(backoff::Error::Transient { retry_after: Some(after), .. }) => {
                Classified::After(after)
            }
            Err(backoff::Error::Transient { retry_after: None, .. }) => Classified::Transient,
            Ok(()) => unreachable!("built from an Err"),
        }
    }

    fn http(status: u16, retry_after: Option<Duration>) -> Error {
        Error::ClientHttpErrorResponse { message: "test", status, retry_after }
    }

    fn closed(status: WebSocketCloseStatus) -> Error {
        Error::WebSocketClosed { status, reason: "test".to_owned() }
    }

    #[test]
    fn retry_after_wins_over_status() {
        let after = Duration::from_secs(7);
        assert_eq!(classify(http(503, Some(after))), Classified::After(after));
        // even for statuses that would otherwise be permanent
        assert_eq!(classify(http(429, Some(after))), Classified::After(after));
        assert_eq!(classify(http(404, Some(after))), Classified::After(after));
    }

    #[test]
    fn only_server_errors_are_retried() {
        assert_eq!(classify(http(500, None)), Classified::Transient);
        assert_eq!(classify(http(503, None)), Classified::Transient);
        assert_eq!(classify(http(429, None)), Classified::Permanent);
        assert_eq!(classify(http(404, None)), Classified::Permanent);
        assert_eq!(classify(http(400, None)), Classified::Permanent);
        // a status we can't even parse is not worth retrying
        assert_eq!(classify(http(999, None)), Classified::Permanent);
    }

    #[test]
    fn websocket_close_status_decides_reconnection() {
        use WebSocketCloseStatus::Known;
        use WebSocketKnownCloseStatus::*;

        for status in [ConnectionClosed, InternalServerError, TlsError] {
            assert_eq!(
                classify(closed(Known(status))),
                Classified::Transient,
                "{status}"
            );
        }
        for status in [
            Normal,
            Gone,
            ProtocolError,
            UnsupportedFrameType,
            MissingStatusCode,
            InvalidFrameData,
            Forbidden,
            FrameTooLarge,
            MissingProtocolExtension,
        ] {
            assert_eq!(
                classify(closed(Known(status))),
                Classified::Permanent,
                "{status}"
            );
        }

        let unknown = WebSocketCloseStatus::Unknown(NonZeroU16::new(4000).unwrap());
        assert_eq!(classify(closed(unknown)), Classified::Permanent);
    }

    #[test]
    fn transport_failures_are_retried() {
        assert_eq!(
            classify(Error::IO { source: io::Error::other("nope") }),
            Classified::Transient
        );
        assert_eq!(classify(Error::Timeout), Classified::Transient);
    }

    #[test]
    fn client_side_failures_are_not_retried() {
        assert_eq!(classify(Error::Decrypt), Classified::Permanent);
        assert_eq!(classify(Error::InvalidCipherKey), Classified::Permanent);
        assert_eq!(classify(Error::InvalidInput("test")), Classified::Permanent);
        assert_eq!(
            classify(Error::InvalidServerResponse("test")),
            Classified::Permanent
        );
    }
}
