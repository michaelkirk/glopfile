#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[allow(clippy::derive_partial_eq_without_eq)]
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/glopfile.websocket.protocol.rs"));
}

use std::convert::Infallible;
use std::mem;
use std::num::NonZeroU16;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use derive_more::From;
use futures::channel::oneshot;
use futures::future::{abortable, AbortHandle, Aborted, Shared};
use futures::FutureExt;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use thiserror::Error;

use crate::error::RetryError;
use crate::util::retry;
use crate::util::spawn_local;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub type DefaultWebSocketConnection = web::WebWebSocketConnection;
    } else {
        pub type DefaultWebSocketConnection = native::NativeWebSocketConnection;
    }
}

#[derive(Clone)]
pub struct WebSocketClient {
    connection: Arc<Mutex<Shared<oneshot::Receiver<WebSocketConnection>>>>,
    _task_handle: Arc<WebSocketTaskHandle>,
}

pub trait WebSocketMessageHandler: Clone + Send + 'static {
    fn handle(&mut self, message: WebSocketMessage) -> ControlFlow<()>;
}

pub struct WebSocketConnection<T: WebSocketConnectionImpl = DefaultWebSocketConnection> {
    connection: Arc<T>,
}

/// A handle to the async task spawned by `WebSocketClient::new`, which is aborted when this struct is dropped.
struct WebSocketTaskHandle {
    handle: AbortHandle,
}

#[async_trait::async_trait(?Send)]
pub trait WebSocketConnectionImpl {
    async fn connect(
        url: &str,
        mut handler: impl WebSocketMessageHandler,
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

impl WebSocketClient {
    // The whole client is single-threaded, so the shared connection needn't be Send or Sync.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(url: String, handler: impl WebSocketMessageHandler) -> Self {
        let (mut connection_tx, connection_rx) = oneshot::channel();
        let connection = Arc::new(Mutex::new(connection_rx.shared()));
        let connection_2 = Arc::clone(&connection);

        let websocket_task = retry(move || {
            let (url, handler, connection) = (url.clone(), handler.clone(), connection_2.clone());
            let (new_connection_tx, new_connection_rx) = oneshot::channel();
            let connection_tx = mem::replace(&mut connection_tx, new_connection_tx);
            async move {
                let connect_result =
                    WebSocketConnection::<DefaultWebSocketConnection>::connect(&url, handler).await;
                if let Err(error) = &connect_result {
                    warn!("error connecting to websocket: {error}");
                }
                let websocket = connect_result?;

                let _ignore = connection_tx.send(websocket.clone());

                let join_result = websocket.join().await.and_then(|()| {
                    warn!("websocket closed by server");
                    Err::<Infallible, _>(WebSocketError::Closed {
                        status: WebSocketKnownCloseStatus::ConnectionClosed.into(),
                        reason: "server closed".into(),
                    })
                });
                if let Err(error) = &join_result {
                    warn!("websocket error: {error}");
                }

                *connection.lock().unwrap() = new_connection_rx.shared();

                // Return an error to potentially trigger reconnect
                join_result.map_err(RetryError::from)
            }
        });
        let (websocket_task, websocket_task_handle) = abortable(websocket_task);
        let task_handle = WebSocketTaskHandle { handle: websocket_task_handle };

        spawn_local(async move {
            match websocket_task.await {
                Ok(Ok(never)) => match never {},
                Ok(Err(error)) => {
                    warn!("websocket fatal error: {error}");
                }
                Err(Aborted) => {
                    debug!("closing websocket");
                }
            }
        });

        Self { connection, _task_handle: Arc::new(task_handle) }
    }

    pub async fn connect(&self) -> Result<WebSocketConnection, WebSocketError> {
        let connection = self.connection.lock().unwrap().clone();
        connection
            .await
            .map_err(|_| WebSocketError::WebSocketClient { source: "websocket thread died".into() })
    }
}

impl<T: WebSocketConnectionImpl> WebSocketConnection<T> {
    pub async fn connect(
        url: &str,
        handler: impl WebSocketMessageHandler,
    ) -> Result<Self, WebSocketError> {
        let connection = Arc::new(T::connect(url, handler).await?);
        Ok(Self { connection })
    }

    pub async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        self.connection.send(message).await
    }

    pub async fn join(&self) -> Result<(), WebSocketError> {
        self.connection.join().await
    }
}

impl<T: WebSocketConnectionImpl> Clone for WebSocketConnection<T> {
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

impl Drop for WebSocketTaskHandle {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
