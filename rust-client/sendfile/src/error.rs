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
    #[error("Invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("Invalid server response: {0}")]
    InvalidServerResponse(&'static str),
}
