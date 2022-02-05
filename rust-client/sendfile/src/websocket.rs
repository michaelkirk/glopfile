use std::ops::ControlFlow;
use std::panic::resume_unwind;
use std::thread::JoinHandle;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use prost::Message;
use reqwest::Url;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use webrtc_sdp::error::SdpParserError;

use crate::Error;

pub(crate) struct WebSocketConnection {
    outgoing_message_tx: mpsc::UnboundedSender<(tungstenite::Message, oneshot::Sender<()>)>,
    thread: Option<JoinHandle<Result<(), Error>>>,
}

pub(crate) use self::protocol::*;

#[derive(Debug, thiserror::Error)]
pub enum ConvertSessionDescriptionError {
    #[error("invalid SDP content: {0}")]
    Parse(#[from] SdpParserError),
    #[error("invalid SessionDescriptionType in protobuf: {0}")]
    InvalidType(i32),
}

mod protocol {
    include!(concat!(env!("OUT_DIR"), "/sendfile.websocket.protocol.rs"));
}

impl WebSocketConnection {
    pub fn connect(
        url: Url,
        mut handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + Send + 'static,
    ) -> Result<Self, Error> {
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
                                debug!("sending websocket message with len {len}", len = message.len());
                                connection.get_mut().send(message).await?;
                                let _ = reply_tx.send(());
                            }
                            None => break,
                        },
                        message = connection.next() => match message {
                            Some(Ok(tungstenite::Message::Text(text))) => {
                                debug!("received websocket text message: {text}");
                            }
                            Some(Ok(tungstenite::Message::Binary(data))) => {
                                let message = WebSocketMessage::decode(Bytes::from(data))
                                    .map_err(|source| Error::InvalidPeerMessage { source: source.into() })?;
                                debug!("received websocket message: {message:?}");
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
        let mut connection = Self { thread: Some(thread), outgoing_message_tx };
        connect_rx
            .blocking_recv()
            .map_err(|_| connection.join().unwrap_err())?;
        Ok(connection)
    }

    pub fn send(&mut self, message: &WebSocketMessage) -> Result<(), Error> {
        let encoded = message.encode_to_vec();
        let (reply_tx, reply_rx) = oneshot::channel();
        self.outgoing_message_tx
            .send((tungstenite::Message::Binary(encoded), reply_tx))
            .map_err(|_| self.join().unwrap_err())?;
        reply_rx
            .blocking_recv()
            .map_err(|_| self.join().unwrap_err())?;
        Ok(())
    }

    fn join(&mut self) -> Result<(), Error> {
        match self.thread.take() {
            Some(thread) => thread
                .join()
                .unwrap_or_else(|panic_payload| resume_unwind(panic_payload)),
            None => Err(Error::Websocket { source: tungstenite::Error::AlreadyClosed }),
        }
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

impl From<datachannel::SessionDescription> for SessionDescription {
    fn from(from: datachannel::SessionDescription) -> Self {
        Self {
            sdp: from.sdp.to_string(),
            sdp_type: SessionDescriptionType::from(from.sdp_type).into(),
        }
    }
}

impl TryFrom<SessionDescription> for datachannel::SessionDescription {
    type Error = ConvertSessionDescriptionError;
    fn try_from(from: SessionDescription) -> Result<Self, Self::Error> {
        let sdp = webrtc_sdp::parse_sdp(&from.sdp, false)?;
        let sdp_type = SessionDescriptionType::from_i32(from.sdp_type)
            .ok_or(Self::Error::InvalidType(from.sdp_type))?;
        Ok(datachannel::SessionDescription { sdp, sdp_type: sdp_type.into() })
    }
}

impl From<SessionDescriptionType> for datachannel::SdpType {
    fn from(from: SessionDescriptionType) -> Self {
        match from {
            SessionDescriptionType::Answer => Self::Answer,
            SessionDescriptionType::Offer => Self::Offer,
            SessionDescriptionType::Pranswer => Self::Pranswer,
            SessionDescriptionType::Rollback => Self::Rollback,
        }
    }
}

impl From<datachannel::SdpType> for SessionDescriptionType {
    fn from(from: datachannel::SdpType) -> Self {
        match from {
            datachannel::SdpType::Answer => Self::Answer,
            datachannel::SdpType::Offer => Self::Offer,
            datachannel::SdpType::Pranswer => Self::Pranswer,
            datachannel::SdpType::Rollback => Self::Rollback,
        }
    }
}

impl From<IceCandidate> for datachannel::IceCandidate {
    fn from(from: IceCandidate) -> Self {
        Self { candidate: from.candidate, mid: from.mid }
    }
}

impl From<datachannel::IceCandidate> for IceCandidate {
    fn from(from: datachannel::IceCandidate) -> Self {
        Self { candidate: from.candidate, mid: from.mid }
    }
}
