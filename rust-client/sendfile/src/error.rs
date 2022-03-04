use crate::websocket::WebSocketError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("API status {status} - {message}")]
    ClientHttpErrorResponse { message: &'static str, status: u16 },
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
    #[error("Invalid server response: {0}")]
    InvalidServerResponse(&'static str),
    #[error("Timed out")]
    Timeout,
}

impl Error {
    pub(crate) fn rtc_err<E>(error: E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::RTCDataChannel { source: error.into() }
    }
}

impl From<WebSocketError> for Error {
    fn from(error: WebSocketError) -> Self {
        match error {
            WebSocketError::ClientHttpErrorResponse { message, status } => {
                Self::ClientHttpErrorResponse { message, status }
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
