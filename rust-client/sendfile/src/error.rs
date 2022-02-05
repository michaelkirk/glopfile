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
    #[error("Websocket error: {source}")]
    Websocket {
        #[source]
        source: tungstenite::Error,
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

impl From<tungstenite::Error> for Error {
    fn from(error: tungstenite::Error) -> Self {
        match error {
            tungstenite::Error::Http(response) => Self::ClientHttpErrorResponse {
                message: "failed to connect to websocket",
                status: response.status().as_u16(),
            },
            tungstenite::Error::Io(source) => Self::IO { source },
            source => Self::Websocket { source },
        }
    }
}

impl Error {
    pub(crate) fn rtc_err<E>(error: E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::RTCDataChannel { source: error.into() }
    }
}
