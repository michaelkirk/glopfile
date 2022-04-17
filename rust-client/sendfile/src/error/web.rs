use sendfile_macros::typescript_error;

use crate::Error;

#[typescript_error]
pub struct HTTPErrorResponseError {
    #[typescript_type = "number"]
    pub status: u16,
}

#[typescript_error]
pub struct WebSocketClosedError {
    #[typescript_type = "number"]
    pub status: u16,
}

#[typescript_error]
pub struct IOError;

#[typescript_error]
pub struct HTTPClientError;

#[typescript_error]
pub struct RTCDataChannelError;

#[typescript_error]
pub struct WebSocketClientError;

#[typescript_error]
pub struct InvalidPeerMessageError;

#[typescript_error]
pub struct InvalidInputError;

#[typescript_error]
pub struct InvalidCipherKeyError;

#[typescript_error]
pub struct DecryptError;

#[typescript_error]
pub struct InvalidServerResponseError;

#[typescript_error]
pub struct TimeoutError;

impl From<Error> for js_sys::Error {
    fn from(from: Error) -> Self {
        let message = from.to_string();
        match from {
            Error::ClientHttpErrorResponse { status, .. } => {
                HTTPErrorResponseError::new(&message, status).into()
            }
            Error::WebSocketClosed { status, .. } => {
                WebSocketClosedError::new(&message, status.into()).into()
            }
            Error::IO { .. } => IOError::new(&message).into(),
            Error::HTTPClient { .. } => HTTPClientError::new(&message).into(),
            Error::RTCDataChannel { .. } => RTCDataChannelError::new(&message).into(),
            Error::WebSocketClient { .. } => WebSocketClientError::new(&message).into(),
            Error::InvalidPeerMessage { .. } => InvalidPeerMessageError::new(&message).into(),
            Error::InvalidInput { .. } => InvalidInputError::new(&message).into(),
            Error::InvalidCipherKey => InvalidCipherKeyError::new(&message).into(),
            Error::Decrypt => DecryptError::new(&message).into(),
            Error::InvalidServerResponse { .. } => InvalidServerResponseError::new(&message).into(),
            Error::Timeout => TimeoutError::new(&message).into(),
        }
    }
}
