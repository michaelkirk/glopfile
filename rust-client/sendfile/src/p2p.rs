#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use instant::{Duration, Instant};
use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};

use bytes::Bytes;
use prost::Message;

use crate::mpsc;
use crate::websocket::WebSocketClient;
use crate::websocket::{
    rtc_signaling_message, web_socket_message, IceCandidate, RtcSignalingMessage,
    SessionDescription, SessionDescriptionType, WebSocketMessage,
};
use crate::Error;

use self::protocol::{downloader_message, uploader_message, DownloaderMessage, UploaderMessage};

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub type DefaultRtc = web::WebRtc;
    } else {
        pub type DefaultRtc = native::NativeRtc;
    }
}

pub struct PeerToPeerClient<RtcTy: Rtc = DefaultRtc> {
    peer_connection: RtcTy::PeerConnection,
    data_channel: <RtcTy::PeerConnection as RtcPeerConnection>::DataChannel,
    signaling: SignalingWebSocket,
    signaling_handler: SignalingMessageHandler,
    rx: mpsc::Receiver<PeerToPeerClientEvent>,
}

#[derive(Clone)]
pub struct SignalingMessageHandler {
    tx: mpsc::Sender<PeerToPeerClientEvent>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("PeerToPeerClient dropped")]
pub struct SignalingMessageHandlerError;

pub trait Rtc {
    type PeerConnection: RtcPeerConnection;

    fn new_peer_connection(
        stun_servers: &[&str],
        tx: mpsc::Sender<PeerToPeerClientEvent>,
    ) -> Result<Self::PeerConnection, Error>;
}

#[async_trait::async_trait(?Send)]
pub trait RtcPeerConnection {
    type DataChannel: RtcDataChannel;

    fn new_data_channel(
        &mut self,
        id: u16,
        label: &str,
        tx: mpsc::Sender<PeerToPeerClientEvent>,
    ) -> Result<Self::DataChannel, Error>;

    async fn create_offer(&mut self) -> Result<(), Error>;
    async fn create_answer(&mut self) -> Result<(), Error>;
    async fn set_remote_description(
        &mut self,
        description: SessionDescription,
    ) -> Result<(), Error>;
    fn local_description_type(&self) -> Option<SessionDescriptionType>;
    async fn add_remote_candidate(&mut self, candidate: IceCandidate) -> Result<(), Error>;
}

pub trait RtcDataChannel {
    fn send(&mut self, message: &[u8]) -> Result<(), Error>;
}

pub trait PeerToPeerClientHandler<RtcTy: Rtc = DefaultRtc> {
    fn data_channel_opened(
        &mut self,
        _client: &mut PeerToPeerClient<RtcTy>,
    ) -> Result<ControlFlow<()>, Error> {
        Ok(Continue(()))
    }

    fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient<RtcTy>,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>, Error>;
}

#[derive(Debug, thiserror::Error)]
#[error("RTC thread died")]
pub(crate) struct RTCThreadDiedError;

#[allow(dead_code)] // inhibit "variant is never constructed" warnings when no implementation is compiled
pub enum PeerToPeerClientEvent {
    OutgoingSignalingMessage(RtcSignalingMessage),
    IncomingSignalingMessage(RtcSignalingMessage),
    DataChannelOpened,
    DataChannelError(Box<dyn std::error::Error + Send + Sync + 'static>),
    DataChannelMessage(Bytes),
}

#[derive(Default)]
struct SignalingWebSocket {
    websocket: Option<WebSocketClient>,
    pending: VecDeque<WebSocketMessage>,
}

struct Timeout {
    deadline: Instant,
    timeout: Duration,
}

pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/sendfile.p2p.protocol.rs"));
}

const DATA_CHANNEL_LABEL: &str = "sendfile";
const DATA_CHANNEL_ID: u16 = 0;
const STUN_SERVERS: &[&str] = &["stun:stun.l.google.com:19302"];

impl<RtcTy: Rtc> PeerToPeerClient<RtcTy> {
    pub fn new() -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel();

        let mut peer_connection = RtcTy::new_peer_connection(STUN_SERVERS, tx.clone())?;
        let data_channel =
            peer_connection.new_data_channel(DATA_CHANNEL_ID, DATA_CHANNEL_LABEL, tx.clone())?;

        Ok(Self {
            peer_connection,
            data_channel,
            rx,
            signaling_handler: SignalingMessageHandler { tx },
            signaling: SignalingWebSocket::default(),
        })
    }

    pub fn signaling_message_handler(&self) -> SignalingMessageHandler {
        self.signaling_handler.clone()
    }

    pub async fn create_offer(&mut self) -> Result<(), Error> {
        debug!("creating RTC offer");
        self.peer_connection.create_offer().await
    }

    pub async fn set_websocket(&mut self, websocket: WebSocketClient) -> Result<(), Error> {
        self.signaling.websocket = Some(websocket);
        self.signaling.flush().await
    }

    pub async fn transfer(
        &mut self,
        handler: &mut impl PeerToPeerClientHandler<RtcTy>,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let mut inactivity_timeout = timeout.map(Timeout::new);
        loop {
            let handler_message = match &inactivity_timeout {
                Some(timeout) => {
                    let handler_rx_res = self.rx.recv_timeout(timeout.remaining()?).await;
                    handler_rx_res.map_err(|error| match error {
                        mpsc::RecvTimeoutError::Disconnected => Error::rtc_err(RTCThreadDiedError),
                        mpsc::RecvTimeoutError::Timeout => Error::Timeout,
                    })?
                }
                None => {
                    let handler_rx_res = self.rx.recv().await;
                    handler_rx_res.map_err(|mpsc::RecvError| Error::rtc_err(RTCThreadDiedError))?
                }
            };

            match handler_message {
                PeerToPeerClientEvent::OutgoingSignalingMessage(message) => {
                    debug!("sending RTC signaling message: {message:?}");
                    self.signaling
                        .send(web_socket_message::Inner::RtcSignaling(message).into())
                        .await?;
                }

                PeerToPeerClientEvent::IncomingSignalingMessage(message) => {
                    debug!("received RTC signaling message: {message:?}");
                    self.handle_incoming_signaling_message(message).await?;
                }

                PeerToPeerClientEvent::DataChannelOpened => {
                    debug!("RTC data channel opened");
                    if let Break(()) = handler.data_channel_opened(self)? {
                        break;
                    }
                }

                PeerToPeerClientEvent::DataChannelError(error) => {
                    warn!("RTC data channel error: {error}");
                    let source = error.into();
                    return Err(Error::RTCDataChannel { source });
                }

                PeerToPeerClientEvent::DataChannelMessage(message_data) => {
                    inactivity_timeout.as_mut().map(Timeout::reset);
                    if let Break(()) = handler.data_channel_message(self, message_data)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_incoming_signaling_message(
        &mut self,
        message: RtcSignalingMessage,
    ) -> Result<(), Error> {
        match message.inner {
            Some(rtc_signaling_message::Inner::SessionDescription(remote_description)) => {
                let local_description_type = self.peer_connection.local_description_type();
                let remote_description_type = remote_description.sdp_type();

                use SessionDescriptionType::{Answer, Offer, Pranswer, Rollback};
                match (local_description_type, remote_description_type) {
                    // receive an offer when we haven't sent one
                    (_local @ None, _remote @ Offer) => {
                        self.peer_connection
                            .set_remote_description(remote_description)
                            .await?;
                        self.peer_connection.create_answer().await?;
                    }

                    // receive an answer to an offer we sent
                    (_local @ Some(Offer), _remote @ Answer) => {
                        self.peer_connection
                            .set_remote_description(remote_description)
                            .await?;
                    }

                    // receive a non-offer message when we haven't sent one
                    (_local @ None, _remote @ (Answer | Pranswer | Rollback)) => {
                        warn!("unexpected SDP {remote_description_type:?} from peer");
                    }

                    // receive a non-answer to an offer we sent
                    (_local @ Some(Offer), _remote @ (Offer | Pranswer | Rollback)) => {
                        warn!("unexpected SDP {remote_description_type:?} from peer after sending an Offer");
                    }

                    // receive any SDP message after we sent an answer
                    (_local @ Some(Answer), _remote @ (Offer | Answer | Pranswer | Rollback)) => {
                        warn!("unexpected SDP {remote_description_type:?} from peer after sending an Answer");
                    }

                    // we don't send pranswer or rollback
                    (
                        _local @ Some(Pranswer | Rollback),
                        _remote @ (Offer | Answer | Pranswer | Rollback),
                    ) => panic!("unexpected local session description {local_description_type:?}"),
                }
            }
            Some(rtc_signaling_message::Inner::IceCandidate(ice_candidate)) => {
                self.peer_connection.add_remote_candidate(ice_candidate).await?;
            }
            None => {
                // Unfortunately, with prost there's no way to log about what message type this actually was.
                warn!("unhandled RTC data channel message type from peer");
            }
        }
        Ok(())
    }

    pub fn send_downloader_message(
        &mut self,
        message_inner: downloader_message::Inner,
    ) -> Result<(), Error> {
        let message = DownloaderMessage { inner: Some(message_inner) };
        let encoded_message = message.encode_to_vec();
        self.data_channel.send(&encoded_message)?;
        Ok(())
    }

    pub fn send_uploader_message(
        &mut self,
        message_inner: uploader_message::Inner,
    ) -> Result<(), Error> {
        let message = UploaderMessage { inner: Some(message_inner) };
        let encoded_message = message.encode_to_vec();
        self.data_channel.send(&encoded_message)?;
        Ok(())
    }
}

impl SignalingMessageHandler {
    pub fn handle(&self, message: RtcSignalingMessage) -> Result<(), SignalingMessageHandlerError> {
        self.tx
            .send(PeerToPeerClientEvent::IncomingSignalingMessage(message))
            .map_err(|_| SignalingMessageHandlerError)
    }
}

impl SignalingWebSocket {
    async fn flush(&mut self) -> Result<(), Error> {
        if let Some(websocket) = &mut self.websocket {
            while let Some(pending) = self.pending.front() {
                websocket.send(pending).await?;
                self.pending.pop_front();
            }
        }
        Ok(())
    }

    async fn send(&mut self, message: WebSocketMessage) -> Result<(), Error> {
        self.flush().await?;
        let result = match &mut self.websocket {
            Some(websocket) => match websocket.send(&message).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.websocket = None;
                    Err(error)
                }
            },
            None => Ok(()),
        };
        self.pending.push_back(message);
        result.map_err(Into::into)
    }
}

impl Timeout {
    fn new(timeout: Duration) -> Self {
        Self { deadline: Self::deadline(timeout), timeout }
    }

    fn deadline(timeout: Duration) -> Instant {
        Instant::now()
            .checked_add(timeout)
            .expect("timeout does not overflow")
    }

    fn remaining(&self) -> Result<Duration, Error> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Error::Timeout)
    }

    fn reset(&mut self) {
        self.deadline = Self::deadline(self.timeout);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Timeout;

    #[test]
    fn zero_timeout() {
        assert!(Timeout::new(Duration::ZERO).remaining().is_err());
    }

    #[test]
    fn timeout_reset() {
        let mut timeout = Timeout::new(Duration::from_secs(600));
        timeout.reset();
        assert!(timeout.remaining().is_ok());
    }
}
