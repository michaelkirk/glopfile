use std::ops::ControlFlow;
use std::panic::resume_unwind;
use std::sync::Mutex;
use std::thread::JoinHandle;

use bytes::Bytes;
use futures::future::{abortable, pending, Abortable, Aborted, Pending};
use futures::never::Never;
use futures::{SinkExt, StreamExt};
use prost::Message;
use scopeguard::guard;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tungstenite::protocol::CloseFrame;

use super::protocol::*;
use super::{WebSocketError, WebSocketMessageHandler};
use crate::util::native::current_thread_block_on;
use crate::util::ResponseExt;

pub struct NativeWebSocketConnection {
    outgoing_message_tx: mpsc::UnboundedSender<(tungstenite::Message, oneshot::Sender<()>)>,
    thread: Mutex<Option<JoinHandle<Result<(), WebSocketError>>>>,
    joiner: Abortable<Pending<Never>>,
}

impl NativeWebSocketConnection {
    fn join_sync(&self) -> Result<(), WebSocketError> {
        let thread = self.thread.lock().unwrap().take();
        match thread {
            Some(thread) => thread
                .join()
                .unwrap_or_else(|panic_payload| resume_unwind(panic_payload)),
            None => Err(tungstenite::Error::AlreadyClosed.into()),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl crate::websocket::WebSocketConnectionImpl for NativeWebSocketConnection {
    async fn connect(
        url: &str,
        mut incoming_message_handler: impl WebSocketMessageHandler,
    ) -> Result<Self, WebSocketError> {
        let url = url.to_string();
        let (connect_tx, connect_rx) = oneshot::channel();
        let (outgoing_message_tx, outgoing_message_rx) =
            mpsc::unbounded_channel::<(tungstenite::Message, oneshot::Sender<()>)>();
        let (joiner, joiner_handle) = abortable(pending());
        let join_guard = guard(joiner_handle, |joiner_handle| joiner_handle.abort());
        let thread = std::thread::spawn(move || {
            let _join_guard = join_guard;
            current_thread_block_on(async {
                let (connection, _response) = tokio_tungstenite::connect_async(url).await?;
                let _ = connect_tx.send(());
                let mut outgoing_message_stream =
                    UnboundedReceiverStream::new(outgoing_message_rx).fuse();
                let mut connection = connection.fuse();
                loop {
                    futures::select! {
                        message = outgoing_message_stream.next() => match message {
                            Some((message, reply_tx)) => {
                                connection.get_mut().send(message).await?;
                                let _ = reply_tx.send(());
                            }
                            None => break,
                        },
                        message = connection.next() => match message {
                            Some(Ok(tungstenite::Message::Text(text))) => {
                                warn!("received unexpected websocket text message: {text}");
                            }
                            Some(Ok(tungstenite::Message::Binary(data))) => {
                                let message = WebSocketMessage::decode(Bytes::from(data))?;
                                if let ControlFlow::Break(()) = incoming_message_handler.handle(message) {
                                    break;
                                }
                            }
                            Some(Ok(tungstenite::Message::Ping(payload))) => {
                                connection.get_mut().send(tungstenite::Message::Pong(payload)).await?;
                            }
                            Some(Ok(tungstenite::Message::Pong(_))) => (),
                            Some(Ok(tungstenite::Message::Close(None))) => break,
                            Some(Ok(tungstenite::Message::Close(Some(CloseFrame { code, reason })))) => {
                                let status = u16::from(code).into();
                                let reason = reason.to_string();
                                return Err(WebSocketError::Closed { status, reason })
                            }
                            Some(Err(error)) => return Err(error.into()),
                            None => break,
                        },
                        complete => break,
                    }
                }
                Ok(())
            })
        });
        let connection = Self { thread: Mutex::new(Some(thread)), outgoing_message_tx, joiner };
        connect_rx
            .await
            .map_err(|_| connection.join_sync().unwrap_err())?;
        Ok(connection)
    }

    async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        let encoded = message.encode_to_vec();
        let (reply_tx, reply_rx) = oneshot::channel();
        self.outgoing_message_tx
            .send((tungstenite::Message::Binary(encoded), reply_tx))
            .map_err(|_| self.join_sync().unwrap_err())?;
        reply_rx.await.map_err(|_| self.join_sync().unwrap_err())?;
        Ok(())
    }

    async fn join(&self) -> Result<(), WebSocketError> {
        let joiner = self.joiner.clone();
        match joiner.await {
            Ok(never) => match never {},
            Err(Aborted) => self.join_sync(),
        }
    }
}

impl From<tungstenite::Error> for WebSocketError {
    fn from(error: tungstenite::Error) -> Self {
        match error {
            tungstenite::Error::Http(response) => Self::ClientHttpErrorResponse {
                message: "failed to connect to websocket",
                status: response.status().as_u16(),
                retry_after: response.retry_after().ok().flatten(),
            },
            tungstenite::Error::Io(source) => Self::IO { source },
            source => Self::WebSocketClient { source: Box::new(source) },
        }
    }
}
