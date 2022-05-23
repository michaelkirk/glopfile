#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use std::collections::VecDeque;
use std::mem;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};
use std::sync::{Arc, Weak};

use bytes::Bytes;
use futures::channel::mpsc;
use futures::StreamExt;
use instant::{Duration, Instant};
use prost::Message;

use crate::util::TimeoutExt;
use crate::websocket::WebSocketClient;
use crate::websocket::{
    rtc_signaling_message, web_socket_message, IceCandidate, RtcSignalingMessage,
    SessionDescription, SessionDescriptionType, WebSocketMessage,
};
use crate::websocket::{WebSocketClient, WebSocketConnection};
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
    connection: Connection<RtcTy>,
    signaling: SignalingWebSocket,
    tx: Weak<mpsc::UnboundedSender<Event>>,
    rx: mpsc::UnboundedReceiver<Event>,
}

#[derive(Clone)]
pub struct SignalingMessageHandler {
    tx: Weak<mpsc::UnboundedSender<Event>>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("PeerToPeerClient dropped")]
pub struct SignalingMessageHandlerError;

#[derive(Clone)]
pub struct PeerConnectionEventHandler {
    connection_id: PeerConnectionId,
    tx: Arc<mpsc::UnboundedSender<Event>>,
}

pub trait Rtc {
    type PeerConnection: RtcPeerConnection;

    fn new_peer_connection(
        stun_servers: &[&str],
        tx: PeerConnectionEventHandler,
    ) -> Result<Self::PeerConnection, Error>;
}

#[async_trait::async_trait(?Send)]
pub trait RtcPeerConnection {
    type DataChannel: RtcDataChannel;

    fn new_data_channel(
        &mut self,
        id: u16,
        label: &str,
        tx: PeerConnectionEventHandler,
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

#[async_trait::async_trait(?Send)]
pub trait PeerToPeerClientHandler<RtcTy: Rtc = DefaultRtc> {
    async fn data_channel_opened(
        &mut self,
        _client: &mut PeerToPeerClient<RtcTy>,
    ) -> Result<ControlFlow<()>, Error> {
        Ok(Continue(()))
    }

    async fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient<RtcTy>,
        message_data: Bytes,
    ) -> Result<ControlFlow<()>, Error>;
}

#[derive(Debug, thiserror::Error)]
#[error("RTC thread died")]
pub(crate) struct RTCThreadDiedError;

#[allow(dead_code)] // inhibit "variant is never constructed" warnings when no implementation is compiled
#[derive(Debug)]
pub enum PeerConnectionEvent {
    OutgoingSignalingMessage(rtc_signaling_message::Inner),
    DataChannelOpened,
    DataChannelError(Box<dyn std::error::Error + Send + Sync + 'static>),
    DataChannelMessage(Bytes),
}

enum Event {
    IncomingSignalingMessage(RtcSignalingMessage),
    Connection {
        id: PeerConnectionId,
        event: PeerConnectionEvent,
    },
}

struct Connection<RtcTy: Rtc> {
    id: PeerConnectionId,
    peer: RtcTy::PeerConnection,
    channel: <RtcTy::PeerConnection as RtcPeerConnection>::DataChannel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerConnectionId(u32);

#[derive(Default)]
struct SignalingWebSocket {
    client: Option<WebSocketClient>,
    websocket: Option<WebSocketConnection>,
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
        let (tx, rx) = mpsc::unbounded();
        let connection_tx = Arc::new(tx);
        let tx = Arc::downgrade(&connection_tx);
        Ok(Self {
            connection: Connection::new(PeerConnectionId::default(), connection_tx)?,
            rx,
            tx,
            signaling: SignalingWebSocket::default(),
        })
    }

    pub fn signaling_message_handler(&self) -> SignalingMessageHandler {
        SignalingMessageHandler { tx: self.tx.clone() }
    }

    pub async fn create_offer(&mut self) -> Result<(), Error> {
        self.connection.peer.create_offer().await
    }

    pub fn set_websocket_client(&mut self, client: WebSocketClient) {
        self.signaling.client = Some(client);
    }

    pub async fn transfer(
        &mut self,
        handler: &mut impl PeerToPeerClientHandler<RtcTy>,
        inactivity_timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let mut inactivity_timeout = inactivity_timeout.map(Timeout::new);
        loop {
            let handler_message = match &inactivity_timeout {
                Some(inactivity_timeout) => {
                    self.rx
                        .next()
                        .timeout(inactivity_timeout.remaining()?)
                        .await?
                }
                None => self.rx.next().await,
            };

            match handler_message.ok_or_else(|| Error::rtc_err(RTCThreadDiedError))? {
                Event::IncomingSignalingMessage(message) => {
                    self.handle_incoming_signaling_message(message).await?;
                }

                Event::Connection { id, event } if id != self.connection.id => {
                    debug!("dropping RTC event from old connection {id:?}: {event:?}");
                }

                Event::Connection {
                    event: PeerConnectionEvent::OutgoingSignalingMessage(message),
                    ..
                } => {
                    self.handle_outgoing_signaling_message(message, inactivity_timeout.as_mut())
                        .await?;
                }

                Event::Connection { event: PeerConnectionEvent::DataChannelOpened, .. } => {
                    info!("RTC data channel opened");
                    if let Break(()) = handler.data_channel_opened(self).await? {
                        break;
                    }
                }

                Event::Connection {
                    event: PeerConnectionEvent::DataChannelError(error), ..
                } => {
                    warn!("RTC data channel error: {error}");
                    return Err(Error::RTCDataChannel { source: error });
                }

                Event::Connection {
                    event: PeerConnectionEvent::DataChannelMessage(message_data),
                    ..
                } => {
                    inactivity_timeout.as_mut().map(Timeout::reset);
                    if let Break(()) = handler.data_channel_message(self, message_data).await? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_outgoing_signaling_message(
        &mut self,
        message: rtc_signaling_message::Inner,
        inactivity_timeout: Option<&mut Timeout>,
    ) -> Result<(), Error> {
        match &message {
            rtc_signaling_message::Inner::SessionDescription(local_description) => {
                let sdp = &local_description.sdp;
                use SessionDescriptionType::{Answer, Offer, Pranswer, Rollback};
                match local_description.sdp_type() {
                    Offer => info!("sending RTC offer to peer: {sdp}"),
                    Answer => info!("sending RTC answer to peer: {sdp}"),
                    sdp_type @ (Pranswer | Rollback) => {
                        panic!("sending unexpected RTC signaling message to peer: {sdp_type:?}");
                    }
                }
            }
            rtc_signaling_message::Inner::IceCandidate(ice_candidate) => {
                match &*ice_candidate.candidate {
                    "" => {
                        debug!("dropping empty RTC ICE candidate");
                        return Ok(());
                    }
                    candidate => info!("sending RTC ICE candidate to peer: {candidate}"),
                }
            }
        }
        self.signaling
            .send(
                web_socket_message::Inner::RtcSignaling(RtcSignalingMessage {
                    inner: Some(message),
                })
                .into(),
                inactivity_timeout,
            )
            .await?;
        Ok(())
    }

    async fn handle_incoming_signaling_message(
        &mut self,
        message: RtcSignalingMessage,
    ) -> Result<(), Error> {
        match message.inner {
            Some(rtc_signaling_message::Inner::SessionDescription(remote_description)) => {
                let local_description_type = self.connection.peer.local_description_type();
                let remote_description_type = remote_description.sdp_type();
                let remote_description_sdp = &remote_description.sdp;

                use SessionDescriptionType::{Answer, Offer, Pranswer, Rollback};
                match (local_description_type, remote_description_type) {
                    // receive an offer when we haven't sent one
                    (_local @ (None | Some(Answer)), _remote @ Offer) => {
                        if let Some(_) = local_description_type {
                            info!("received new RTC offer from peer; resetting connection: {remote_description_sdp}");
                            let connection_id = self.connection.id.next();
                            let tx = self
                                .tx
                                .upgrade()
                                .ok_or_else(|| Error::rtc_err(RTCThreadDiedError))?;
                            self.connection = Connection::new(connection_id, tx)?;
                        } else {
                            info!("received RTC offer from peer: {remote_description_sdp}");
                        }
                        self.connection
                            .peer
                            .set_remote_description(remote_description)
                            .await?;
                        self.connection.peer.create_answer().await?;
                    }

                    // receive an answer to an offer we sent
                    (_local @ Some(Offer), _remote @ Answer) => {
                        info!("received RTC answer from peer: {remote_description_sdp}");
                        self.connection
                            .peer
                            .set_remote_description(remote_description)
                            .await?;
                    }

                    // receive a non-offer message when we haven't sent one
                    (_local @ None, _remote @ (Answer | Pranswer | Rollback)) => {
                        warn!("unexpected RTC SDP {remote_description_type:?} from peer: {remote_description_sdp}");
                    }

                    // receive a non-answer to an offer we sent
                    (_local @ Some(Offer), _remote @ (Offer | Pranswer | Rollback)) => {
                        warn!("unexpected RTC SDP {remote_description_type:?} from peer after sending an Offer: \
                               {remote_description_sdp}");
                    }

                    // receive a non-offer after we sent an answer
                    (_local @ Some(Answer), _remote @ (Answer | Pranswer | Rollback)) => {
                        warn!("unexpected RTC SDP {remote_description_type:?} from peer after sending an Answer: \
                               {remote_description_sdp}");
                    }

                    // we don't send pranswer or rollback
                    (
                        _local @ Some(Pranswer | Rollback),
                        _remote @ (Offer | Answer | Pranswer | Rollback),
                    ) => panic!(
                        "unexpected RTC local session description {local_description_type:?}"
                    ),
                }
            }
            Some(rtc_signaling_message::Inner::IceCandidate(ice_candidate)) => {
                match &*ice_candidate.candidate {
                    "" => warn!("dropping empty RTC ICE candidate from peer"),
                    candidate => {
                        info!("adding RTC ICE candidate from peer: {candidate}");
                        self.connection
                            .peer
                            .add_remote_candidate(ice_candidate)
                            .await?;
                    }
                }
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
        self.connection.channel.send(&encoded_message)?;
        Ok(())
    }

    pub fn send_uploader_message(
        &mut self,
        message_inner: uploader_message::Inner,
    ) -> Result<(), Error> {
        let message = UploaderMessage { inner: Some(message_inner) };
        let encoded_message = message.encode_to_vec();
        self.connection.channel.send(&encoded_message)?;
        Ok(())
    }
}

impl SignalingMessageHandler {
    pub fn handle(&self, message: RtcSignalingMessage) -> Result<(), SignalingMessageHandlerError> {
        self.tx
            .upgrade()
            .ok_or(SignalingMessageHandlerError)?
            .unbounded_send(Event::IncomingSignalingMessage(message))
            .map_err(|_| SignalingMessageHandlerError)
    }
}

impl PeerConnectionEventHandler {
    pub fn handle(&self, event: PeerConnectionEvent) {
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self
            .tx
            .unbounded_send(Event::Connection { id: self.connection_id, event })
            .map_err(|_| SignalingMessageHandlerError);
    }
}

impl<RtcTy: Rtc> Connection<RtcTy> {
    fn new(id: PeerConnectionId, tx: Arc<mpsc::UnboundedSender<Event>>) -> Result<Self, Error> {
        let handler = PeerConnectionEventHandler { connection_id: id, tx };
        let mut peer_connection = RtcTy::new_peer_connection(STUN_SERVERS, handler.clone())?;
        let data_channel =
            peer_connection.new_data_channel(DATA_CHANNEL_ID, DATA_CHANNEL_LABEL, handler)?;
        Ok(Self { id, peer: peer_connection, channel: data_channel })
    }
}

impl PeerConnectionId {
    fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("connection id overflow"))
    }
}

impl SignalingWebSocket {
    async fn flush(&mut self, mut timeout: Option<&mut Timeout>) -> Result<(), Error> {
        if let Some(client) = &mut self.client {
            while let Some(pending) = self.pending.front() {
                let websocket = match mem::take(&mut self.websocket) {
                    Some(websocket) => websocket,
                    None => match &mut timeout {
                        Some(timeout) => client
                            .connect()
                            .timeout(timeout.remaining()?)
                            .await
                            .map_err(Error::rtc_err)??,
                        None => client.connect().await?,
                    },
                };
                match websocket.send(pending).await {
                    Ok(()) => {
                        self.websocket = Some(websocket);
                    }
                    Err(error) => {
                        warn!("error sending on websocket: {error}");
                    }
                }
                self.pending.pop_front();
            }
        }
        Ok(())
    }

    async fn send(
        &mut self,
        message: WebSocketMessage,
        timeout: Option<&mut Timeout>,
    ) -> Result<(), Error> {
        self.pending.push_back(message);
        self.flush(timeout).await
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
