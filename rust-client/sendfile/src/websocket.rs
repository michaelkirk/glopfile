#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/sendfile.websocket.protocol.rs"));
}

use std::num::NonZeroU16;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use derive_more::From;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use thiserror::Error;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub type DefaultWebSocketConnection = web::WebWebSocketConnection;
    } else {
        pub type DefaultWebSocketConnection = native::NativeWebSocketConnection;
    }
}

pub struct WebSocketClient<T: WebSocketConnection = DefaultWebSocketConnection> {
    connection: Arc<T>,
}

#[async_trait::async_trait(?Send)]
pub trait WebSocketConnection {
    async fn connect(
        url: &str,
        mut handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<Self, WebSocketError>
    where
        Self: Sized;
    async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError>;
    async fn join(&self) -> Result<(), WebSocketError>;
}

pub(crate) use self::protocol::*;

#[derive(Debug, Error)]
pub enum WebSocketError {
    #[error("API status {status} - {message}")]
    ClientHttpErrorResponse {
        message: &'static str,
        status: u16,
        retry_after: Option<Duration>,
    },
    #[error("WebSocket closed with status {status}: {reason}")]
    Closed {
        status: WebSocketCloseStatus,
        reason: String,
    },
    #[error("IO Error: {source}")]
    IO {
        #[from]
        source: std::io::Error,
    },
    #[error("Invalid message received on WebSocket: {source}")]
    InvalidMessage {
        #[from]
        source: prost::DecodeError,
    },
    #[error("WebSocket Client error: {source}")]
    WebSocketClient {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[derive(Clone, Copy, Debug, Error, From, PartialEq, Eq, PartialOrd, Ord)]
pub enum WebSocketCloseStatus {
    #[error("{0}")]
    Known(WebSocketKnownCloseStatus),
    #[error("{0} (unknown)")]
    Unknown(NonZeroU16),
}

impl From<u16> for WebSocketCloseStatus {
    fn from(from: u16) -> Self {
        match from.try_into() {
            Ok(known) => Self::Known(known),
            Err(_) => match NonZeroU16::new(from) {
                Some(unknown) => Self::Unknown(unknown),
                None => Self::Known(WebSocketKnownCloseStatus::MissingStatusCode),
            },
        }
    }
}

impl From<WebSocketCloseStatus> for u16 {
    fn from(from: WebSocketCloseStatus) -> Self {
        match from {
            WebSocketCloseStatus::Known(known) => known.into(),
            WebSocketCloseStatus::Unknown(unknown) => unknown.into(),
        }
    }
}

#[derive(
    Clone, Copy, Debug, Error, PartialEq, Eq, PartialOrd, Ord, IntoPrimitive, TryFromPrimitive,
)]
#[repr(u16)]
pub enum WebSocketKnownCloseStatus {
    #[error("1000 (normal)")]
    Normal = 1000,
    #[error("1001 (gone)")]
    Gone = 1001,
    #[error("1002 (protocol error)")]
    ProtocolError = 1002,
    #[error("1003 (unsupported frame type)")]
    UnsupportedFrameType = 1003,
    #[error("1005 (missing status code)")]
    MissingStatusCode = 1005,
    #[error("1006 (connection closed)")]
    ConnectionClosed = 1006,
    #[error("1007 (invalid frame data)")]
    InvalidFrameData = 1007,
    #[error("1008 (forbidden)")]
    Forbidden = 1008,
    #[error("1009 (frame too large)")]
    FrameTooLarge = 1009,
    #[error("1010 (missing protocol extension)")]
    MissingProtocolExtension = 1010,
    #[error("1011 (internal server error)")]
    InternalServerError = 1011,
    #[error("1015 (TLS error)")]
    TlsError = 1015,
}

impl<T: WebSocketConnection> WebSocketClient<T> {
    pub async fn connect(
        url: &str,
        handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<Self, WebSocketError> {
        let connection = Arc::new(T::connect(url, handle_incoming_message).await?);
        Ok(Self { connection })
    }

    pub async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        self.connection.send(message).await
    }

    pub async fn join(&self) -> Result<(), WebSocketError> {
        self.connection.join().await
    }
}

impl<T: WebSocketConnection> Clone for WebSocketClient<T> {
    fn clone(&self) -> Self {
        Self { connection: Arc::clone(&self.connection) }
    }
}

impl From<web_socket_message::Inner> for WebSocketMessage {
    fn from(inner: web_socket_message::Inner) -> Self {
        Self { inner: Some(inner) }
    }
}

impl From<rtc_signaling_message::Inner> for RtcSignalingMessage {
    fn from(inner: rtc_signaling_message::Inner) -> Self {
        Self { inner: Some(inner) }
    }
}
