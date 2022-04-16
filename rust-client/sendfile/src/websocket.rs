#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/sendfile.websocket.protocol.rs"));
}

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error("API status {status} - {message}")]
    ClientHttpErrorResponse {
        message: &'static str,
        status: u16,
        retry_after: Option<Duration>,
    },
    #[error("WebSocket closed")]
    Closed,
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
