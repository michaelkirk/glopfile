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

/// An error on its way out of a retry attempt.
///
/// Most errors classify themselves via [`Error::retriability`]; the constructors
/// here are for the few call sites that know better than the general rule.
#[derive(Debug)]
pub(crate) struct RetryError {
    error: Error,
    override_: Option<Retriability>,
}

impl RetryError {
    /// Give up regardless of what the error would otherwise say.
    pub(crate) fn permanent(error: impl Into<Error>) -> Self {
        Self { error: error.into(), override_: Some(Retriability::Permanent) }
    }

    /// Retry regardless of what the error would otherwise say.
    pub(crate) fn transient(error: impl Into<Error>) -> Self {
        Self { error: error.into(), override_: Some(Retriability::Transient) }
    }

    pub(crate) fn retriability(&self) -> Retriability {
        self.override_.unwrap_or_else(|| self.error.retriability())
    }
}

impl From<RetryError> for Error {
    fn from(from: RetryError) -> Self {
        from.error
    }
}

macro_rules! retry_error_from {
    ($($source:ty),* $(,)?) => {
        $(impl From<$source> for RetryError {
            fn from(error: $source) -> Self {
                Self { error: error.into(), override_: None }
            }
        })*
    };
}
retry_error_from!(
    Error,
    reqwest::Error,
    std::io::Error,
    TimeoutError,
    WebSocketError
);

/// How the retry loop should treat an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Retriability {
    /// Retrying will not help.
    Permanent,
    /// Retry on the usual backoff schedule.
    Transient,
    /// Retry, but not before the server-requested delay has elapsed.
    After(Duration),
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

impl Error {
    pub(crate) fn retriability(&self) -> Retriability {
        match self {
            Error::ClientHttpErrorResponse { retry_after: Some(retry_after), .. } => {
                Retriability::After(*retry_after)
            }
            Error::ClientHttpErrorResponse { status, retry_after: None, .. } => {
                match StatusCode::from_u16(*status) {
                    Ok(status) if status.is_server_error() => Retriability::Transient,
                    Ok(_status) => Retriability::Permanent,
                    Err(status_error) => {
                        warn!(
                            "invalid HTTP status code {status} from server in response {self}: \
                             {status_error}"
                        );
                        Retriability::Permanent
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
                    | ProtocolError => Retriability::Permanent,
                    ConnectionClosed | InternalServerError | TlsError => Retriability::Transient,
                }
            }
            Error::HTTPClient { .. } | Error::IO { .. } | Error::Timeout => Retriability::Transient,
            _ => Retriability::Permanent,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io;
    use std::num::NonZeroU16;

    use super::*;

    use Retriability as Classified;

    fn classify(error: Error) -> Classified {
        error.retriability()
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
        assert_eq!(classify(http(409, None)), Classified::Permanent);
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
    fn call_sites_can_override_the_table() {
        // a spurious 409 is retried although 4xx is otherwise permanent
        assert_eq!(classify(http(409, None)), Classified::Permanent);
        assert_eq!(
            RetryError::transient(http(409, None)).retriability(),
            Classified::Transient
        );

        // a failed write to the output file is not worth re-downloading for
        let disk_error = || Error::IO { source: io::Error::other("disk full") };
        assert_eq!(classify(disk_error()), Classified::Transient);
        assert_eq!(
            RetryError::permanent(disk_error()).retriability(),
            Classified::Permanent
        );
    }

    #[test]
    fn errors_classify_themselves_unless_overridden() {
        let error = || http(503, None);
        assert_eq!(
            RetryError::from(error()).retriability(),
            Classified::Transient
        );
        assert_eq!(classify(error()), RetryError::from(error()).retriability());
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
