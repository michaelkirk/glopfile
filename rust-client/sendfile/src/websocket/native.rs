use std::ops::ControlFlow;
use std::panic::resume_unwind;
use std::sync::Mutex;
use std::thread::JoinHandle;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use prost::Message;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::protocol::*;
use super::WebSocketError;

pub struct NativeWebSocketConnection {
    outgoing_message_tx: mpsc::UnboundedSender<(tungstenite::Message, oneshot::Sender<()>)>,
    thread: Mutex<Option<JoinHandle<Result<(), WebSocketError>>>>,
}

impl NativeWebSocketConnection {
    fn join(&self) -> Result<(), WebSocketError> {
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
impl crate::websocket::WebSocketConnection for NativeWebSocketConnection {
    async fn connect(
        url: &str,
        mut handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<Self, WebSocketError> {
        let url = url.to_string();
        let (connect_tx, connect_rx) = oneshot::channel();
        let (outgoing_message_tx, outgoing_message_rx) =
            mpsc::unbounded_channel::<(tungstenite::Message, oneshot::Sender<()>)>();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let (connection, _response) =
                runtime.block_on(tokio_tungstenite::connect_async(url))?;
            let _ = connect_tx.send(());

            runtime.block_on(async {
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
                                if let ControlFlow::Break(()) = handle_incoming_message(message) {
                                    break;
                                }
                            }
                            Some(Ok(tungstenite::Message::Ping(payload))) => {
                                connection.get_mut().send(tungstenite::Message::Pong(payload)).await?;
                            }
                            Some(Ok(tungstenite::Message::Pong(_))) => (),
                            Some(Ok(tungstenite::Message::Close(frame))) => {
                                info!("websocket closed: {frame:?}");
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
        let connection = Self { thread: Mutex::new(Some(thread)), outgoing_message_tx };
        connect_rx
            .await
            .map_err(|_| connection.join().unwrap_err())?;
        Ok(connection)
    }

    async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        let encoded = message.encode_to_vec();
        let (reply_tx, reply_rx) = oneshot::channel();
        self.outgoing_message_tx
            .send((tungstenite::Message::Binary(encoded), reply_tx))
            .map_err(|_| self.join().unwrap_err())?;
        reply_rx.await.map_err(|_| self.join().unwrap_err())?;
        Ok(())
    }
}

impl From<tungstenite::Error> for WebSocketError {
    fn from(error: tungstenite::Error) -> Self {
        match error {
            tungstenite::Error::Http(response) => Self::ClientHttpErrorResponse {
                message: "failed to connect to websocket",
                status: response.status().as_u16(),
            },
            tungstenite::Error::Io(source) => Self::IO { source },
            source => Self::WebSocketClient { source: Box::new(source) },
        }
    }
}
