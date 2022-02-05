use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::ops::ControlFlow::{Break, Continue};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use datachannel::{
    DataChannelInit, RtcConfig, RtcDataChannel, RtcPeerConnection, SessionDescription,
};
use prost::Message;

use crate::websocket::{
    rtc_signaling_message, web_socket_message, RtcSignalingMessage, WebSocketConnection,
    WebSocketMessage,
};
use crate::Error;

use self::protocol::{downloader_message, uploader_message, DownloaderMessage, UploaderMessage};

pub(crate) struct PeerToPeerClient {
    peer_connection: Box<RtcPeerConnection<PeerConnectionHandler>>,
    data_channel: Box<RtcDataChannel<DataChannelHandler>>,
    signaling: SignalingWebSocket,
    signaling_handler: SignalingMessageHandler,
    rx: mpsc::Receiver<PeerToPeerClientEvent>,
}

#[derive(Clone)]
pub(crate) struct SignalingMessageHandler {
    tx: mpsc::Sender<PeerToPeerClientEvent>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("PeerToPeerClient dropped")]
pub(crate) struct SignalingMessageHandlerError;

pub(crate) trait PeerToPeerClientHandler {
    fn data_channel_opened(
        &mut self,
        _client: &mut PeerToPeerClient,
    ) -> Result<ControlFlow<()>, Error> {
        Ok(Continue(()))
    }

    fn data_channel_message(
        &mut self,
        client: &mut PeerToPeerClient,
        message_data: BytesMut,
    ) -> Result<ControlFlow<()>, Error>;
}

#[derive(Debug, thiserror::Error)]
#[error("RTC thread died")]
pub(crate) struct RTCThreadDiedError;

struct PeerConnectionHandler {
    tx: mpsc::Sender<PeerToPeerClientEvent>,
}

struct DataChannelHandler {
    tx: mpsc::Sender<PeerToPeerClientEvent>,
}

enum PeerToPeerClientEvent {
    OutgoingSignalingMessage(RtcSignalingMessage),
    IncomingSignalingMessage(RtcSignalingMessage),
    DataChannelOpened,
    DataChannelError(String),
    DataChannelMessage(BytesMut),
}

#[derive(Default)]
struct SignalingWebSocket {
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

impl PeerToPeerClient {
    pub fn new() -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel();

        let peer_connection_handler = PeerConnectionHandler { tx: tx.clone() };
        let data_channel_handler = DataChannelHandler { tx: tx.clone() };

        let mut rtc_config = RtcConfig::new(STUN_SERVERS);
        rtc_config.disable_auto_negotiation = true;
        let mut peer_connection =
            RtcPeerConnection::new(&rtc_config, peer_connection_handler).map_err(Error::rtc_err)?;

        let data_channel_init = DataChannelInit::default()
            .negotiated()
            .manual_stream()
            .stream(DATA_CHANNEL_ID);
        let data_channel = peer_connection
            .create_data_channel_ex(DATA_CHANNEL_LABEL, data_channel_handler, &data_channel_init)
            .map_err(Error::rtc_err)?;

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

    pub fn create_offer(&mut self) -> Result<(), Error> {
        debug!("creating RTC offer");
        self.peer_connection
            .set_local_description(datachannel::SdpType::Offer)
            .map_err(Error::rtc_err)
    }

    pub fn set_websocket(&mut self, websocket: WebSocketConnection) -> Result<(), Error> {
        self.signaling.websocket = Some(websocket);
        self.signaling.flush()
    }

    pub fn transfer(
        &mut self,
        handler: &mut impl PeerToPeerClientHandler,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        let mut inactivity_timeout = timeout.map(Timeout::new);
        loop {
            let handler_message = match &inactivity_timeout {
                Some(timeout) => {
                    let handler_rx_res = self.rx.recv_timeout(timeout.remaining()?);
                    handler_rx_res.map_err(|error| match error {
                        mpsc::RecvTimeoutError::Disconnected => Error::rtc_err(RTCThreadDiedError),
                        mpsc::RecvTimeoutError::Timeout => Error::Timeout,
                    })?
                }
                None => {
                    let handler_rx_res = self.rx.recv();
                    handler_rx_res.map_err(|mpsc::RecvError| Error::rtc_err(RTCThreadDiedError))?
                }
            };

            match handler_message {
                PeerToPeerClientEvent::OutgoingSignalingMessage(message) => {
                    debug!("sending RTC signaling message: {message:?}");
                    self.signaling
                        .send(web_socket_message::Inner::RtcSignaling(message).into())?;
                }

                PeerToPeerClientEvent::IncomingSignalingMessage(message) => {
                    debug!("received RTC signaling message: {message:?}");
                    self.handle_incoming_signaling_message(message)?;
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

    fn handle_incoming_signaling_message(
        &mut self,
        message: RtcSignalingMessage,
    ) -> Result<(), Error> {
        match message.inner {
            Some(rtc_signaling_message::Inner::SessionDescription(remote_description)) => {
                let local_description = self.peer_connection.local_description();
                let local_description_type = local_description.as_ref().map(|sdp| &sdp.sdp_type);

                let remote_description: Result<SessionDescription, _> =
                    remote_description.try_into();
                let remote_description = remote_description
                    .map_err(|error| Error::InvalidPeerMessage { source: error.into() })?;
                let remote_description_type = &remote_description.sdp_type;

                use datachannel::SdpType::{Answer, Offer, Pranswer, Rollback};
                match (local_description_type, remote_description_type) {
                    // receive an offer when we haven't sent one
                    (_local @ None, _remote @ Offer) => {
                        self.peer_connection
                            .set_remote_description(&remote_description)
                            .map_err(Error::rtc_err)?;
                        self.peer_connection
                            .set_local_description(datachannel::SdpType::Answer)
                            .map_err(Error::rtc_err)?;
                    }

                    // receive an answer to an offer we sent
                    (_local @ Some(Offer), _remote @ Answer) => {
                        self.peer_connection
                            .set_remote_description(&remote_description)
                            .map_err(Error::rtc_err)?;
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
                self.peer_connection
                    .add_remote_candidate(&ice_candidate.into())
                    .map_err(Error::rtc_err)?;
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
        self.data_channel
            .send(&encoded_message)
            .map_err(Error::rtc_err)?;
        Ok(())
    }

    pub fn send_uploader_message(
        &mut self,
        message_inner: uploader_message::Inner,
    ) -> Result<(), Error> {
        let message = UploaderMessage { inner: Some(message_inner) };
        let encoded_message = message.encode_to_vec();
        self.data_channel
            .send(&encoded_message)
            .map_err(Error::rtc_err)?;
        Ok(())
    }
}

impl SignalingMessageHandler {
    pub(crate) fn handle(
        &self,
        message: RtcSignalingMessage,
    ) -> Result<(), SignalingMessageHandlerError> {
        self.tx
            .send(PeerToPeerClientEvent::IncomingSignalingMessage(message))
            .map_err(|_| SignalingMessageHandlerError)
    }
}

struct NoopDataChannelHandler;
impl datachannel::DataChannelHandler for NoopDataChannelHandler {}

impl datachannel::PeerConnectionHandler for PeerConnectionHandler {
    type DCH = NoopDataChannelHandler;

    fn data_channel_handler(&mut self) -> Self::DCH {
        NoopDataChannelHandler
    }

    fn on_description(&mut self, session_description: datachannel::SessionDescription) {
        debug!("new RTC session description: {session_description:?}");
        let signaling_message = RtcSignalingMessage {
            inner: Some(rtc_signaling_message::Inner::SessionDescription(
                session_description.into(),
            )),
        };
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self
            .tx
            .send(PeerToPeerClientEvent::OutgoingSignalingMessage(
                signaling_message,
            ));
    }

    fn on_candidate(&mut self, candidate: datachannel::IceCandidate) {
        debug!("new RTC ice candidate: {candidate:?}");
        let signaling_message = RtcSignalingMessage {
            inner: Some(rtc_signaling_message::Inner::IceCandidate(candidate.into())),
        };
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self
            .tx
            .send(PeerToPeerClientEvent::OutgoingSignalingMessage(
                signaling_message,
            ));
    }

    fn on_connection_state_change(&mut self, state: datachannel::ConnectionState) {
        debug!("RTC connection state changed: {state:?}");
    }

    fn on_gathering_state_change(&mut self, state: datachannel::GatheringState) {
        debug!("RTC candidate gathering state changed: {state:?}");
    }

    fn on_signaling_state_change(&mut self, state: datachannel::SignalingState) {
        debug!("RTC signaling state changed: {state:?}");
    }

    fn on_data_channel(&mut self, data_channel: Box<RtcDataChannel<Self::DCH>>) {
        let stream = data_channel.stream();
        let label = data_channel.label();
        panic!("unexpected RTC data channel stream {stream} received: {label}");
    }
}

impl datachannel::DataChannelHandler for DataChannelHandler {
    fn on_open(&mut self) {
        debug!("RTC data channel opened");
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self.tx.send(PeerToPeerClientEvent::DataChannelOpened);
    }

    fn on_closed(&mut self) {
        debug!("RTC data channel closed");
    }

    fn on_error(&mut self, error: &str) {
        debug!("RTC data channel error: {error}");
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self
            .tx
            .send(PeerToPeerClientEvent::DataChannelError(error.into()));
    }

    fn on_message(&mut self, msg: &[u8]) {
        let len = msg.len();
        debug!("RTC data channel message received of len {len}");
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        let _ignore = self
            .tx
            .send(PeerToPeerClientEvent::DataChannelMessage(msg.into()));
    }

    fn on_buffered_amount_low(&mut self) {
        debug!("RTC data channel buffer now empty");
    }

    fn on_available(&mut self) {
        debug!("RTC data channel now has available data");
    }
}

impl SignalingWebSocket {
    fn flush(&mut self) -> Result<(), Error> {
        if let Some(websocket) = &mut self.websocket {
            while let Some(pending) = self.pending.front() {
                websocket.send(pending)?;
                self.pending.pop_front();
            }
        }
        Ok(())
    }

    fn send(&mut self, message: WebSocketMessage) -> Result<(), Error> {
        self.flush()?;
        let result = match &mut self.websocket {
            Some(websocket) => match websocket.send(&message) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.websocket = None;
                    Err(error)
                }
            },
            None => Ok(()),
        };
        self.pending.push_back(message);
        result
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
