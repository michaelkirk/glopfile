use datachannel::{
    ConnectionState, DataChannelHandler, DataChannelInfo, DataChannelInit, GatheringState,
    IceCandidate, PeerConnectionHandler, RtcConfig, RtcDataChannel, RtcPeerConnection, SdpType,
    SessionDescription, SignalingState,
};
use webrtc_sdp::error::SdpParserError;

use crate::{websocket, Error};

use super::{PeerConnectionEvent, PeerConnectionEventHandler};

pub enum NativeRtc {}

pub struct NativePeerConnection {
    rtc: Box<RtcPeerConnection<NativePeerConnectionHandler>>,
    local_description_type: Option<websocket::SessionDescriptionType>,
}

pub struct NativeDataChannel {
    rtc: Box<RtcDataChannel<NativeDataChannelHandler>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertSessionDescriptionError {
    #[error("invalid SDP content: {0}")]
    Parse(#[from] SdpParserError),
    #[error("invalid SessionDescriptionType in protobuf: {0}")]
    InvalidType(i32),
}

struct NativePeerConnectionHandler {
    tx: PeerConnectionEventHandler,
}

struct NativeDataChannelHandler {
    tx: PeerConnectionEventHandler,
}

impl super::Rtc for NativeRtc {
    type PeerConnection = NativePeerConnection;

    fn new_peer_connection(
        stun_servers: &[&str],
        tx: PeerConnectionEventHandler,
    ) -> Result<Self::PeerConnection, Error> {
        let peer_connection_handler = NativePeerConnectionHandler { tx };

        let mut rtc_config = RtcConfig::new(stun_servers);
        rtc_config.disable_auto_negotiation = true;

        let peer_connection =
            RtcPeerConnection::new(&rtc_config, peer_connection_handler).map_err(Error::rtc_err)?;

        Ok(NativePeerConnection { rtc: peer_connection, local_description_type: None })
    }
}

#[async_trait::async_trait(?Send)]
impl super::RtcPeerConnection for NativePeerConnection {
    type DataChannel = NativeDataChannel;

    fn new_data_channel(
        &mut self,
        id: u16,
        label: &str,
        tx: PeerConnectionEventHandler,
    ) -> Result<Self::DataChannel, Error> {
        let data_channel_handler = NativeDataChannelHandler { tx };
        let data_channel_init = DataChannelInit::default()
            .negotiated()
            .manual_stream()
            .stream(id);
        let data_channel = self
            .rtc
            .create_data_channel_ex(label, data_channel_handler, &data_channel_init)
            .map_err(Error::rtc_err)?;

        Ok(NativeDataChannel { rtc: data_channel })
    }

    async fn create_offer(&mut self) -> Result<(), Error> {
        self.rtc
            .set_local_description(SdpType::Offer)
            .map_err(Error::rtc_err)?;
        self.local_description_type = Some(websocket::SessionDescriptionType::Offer);
        Ok(())
    }

    async fn create_answer(&mut self) -> Result<(), Error> {
        self.rtc
            .set_local_description(SdpType::Answer)
            .map_err(Error::rtc_err)?;
        self.local_description_type = Some(websocket::SessionDescriptionType::Answer);
        Ok(())
    }

    async fn set_remote_description(
        &mut self,
        session_description: websocket::SessionDescription,
    ) -> Result<(), Error> {
        let session_description = (&session_description).try_into().map_err(Error::rtc_err)?;
        self.rtc
            .set_remote_description(&session_description)
            .map_err(Error::rtc_err)
    }

    fn local_description_type(&self) -> Option<websocket::SessionDescriptionType> {
        self.local_description_type
    }

    async fn add_remote_candidate(
        &mut self,
        candidate: websocket::IceCandidate,
    ) -> Result<(), Error> {
        let candidate = IceCandidate::from(candidate);
        self.rtc
            .add_remote_candidate(&candidate)
            .map_err(Error::rtc_err)?;
        Ok(())
    }
}

impl super::RtcDataChannel for NativeDataChannel {
    fn send(&mut self, message: &[u8]) -> Result<(), Error> {
        self.rtc.send(message).map_err(Error::rtc_err)?;
        Ok(())
    }
}

struct NoopDataChannelHandler;
impl DataChannelHandler for NoopDataChannelHandler {}

impl PeerConnectionHandler for NativePeerConnectionHandler {
    type DCH = NoopDataChannelHandler;

    fn data_channel_handler(&mut self, _info: DataChannelInfo) -> Self::DCH {
        NoopDataChannelHandler
    }

    fn on_description(&mut self, session_description: SessionDescription) {
        self.tx
            .handle(PeerConnectionEvent::OutgoingSignalingMessage(
                websocket::rtc_signaling_message::Inner::SessionDescription(
                    (&session_description).into(),
                ),
            ));
    }

    fn on_candidate(&mut self, candidate: IceCandidate) {
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        self.tx
            .handle(PeerConnectionEvent::OutgoingSignalingMessage(
                websocket::rtc_signaling_message::Inner::IceCandidate(candidate.into()),
            ));
    }

    fn on_connection_state_change(&mut self, state: ConnectionState) {
        info!("RTC connection state changed: {state:?}");
    }

    fn on_gathering_state_change(&mut self, state: GatheringState) {
        info!("RTC candidate gathering state changed: {state:?}");
    }

    fn on_signaling_state_change(&mut self, state: SignalingState) {
        info!("RTC signaling state changed: {state:?}");
    }

    fn on_data_channel(&mut self, data_channel: Box<RtcDataChannel<Self::DCH>>) {
        let stream = data_channel.stream();
        let label = data_channel.label();
        panic!("unexpected RTC data channel stream {stream} received: {label}");
    }
}

impl DataChannelHandler for NativeDataChannelHandler {
    fn on_open(&mut self) {
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        self.tx.handle(PeerConnectionEvent::DataChannelOpened);
    }

    fn on_closed(&mut self) {
        info!("RTC data channel closed");
    }

    fn on_error(&mut self, error: &str) {
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        self.tx
            .handle(PeerConnectionEvent::DataChannelError(error.into()));
    }

    fn on_message(&mut self, msg: &[u8]) {
        let len = msg.len();
        trace!("RTC data channel message received of len {len}");
        // an error sending to the main thread should mean the current RTC thread is going to shut down anyway
        self.tx
            .handle(PeerConnectionEvent::DataChannelMessage(msg.to_vec().into()));
    }

    fn on_buffered_amount_low(&mut self) {
        trace!("RTC data channel buffer now empty");
    }

    fn on_available(&mut self) {
        trace!("RTC data channel now has available data");
    }
}

impl From<&SessionDescription> for websocket::SessionDescription {
    fn from(from: &SessionDescription) -> Self {
        Self {
            sdp: from.sdp.to_string(),
            sdp_type: websocket::SessionDescriptionType::from(&from.sdp_type).into(),
        }
    }
}

impl TryFrom<&websocket::SessionDescription> for SessionDescription {
    type Error = ConvertSessionDescriptionError;
    fn try_from(from: &websocket::SessionDescription) -> Result<Self, Self::Error> {
        let sdp = webrtc_sdp::parse_sdp(&from.sdp, false)?;
        let sdp_type = websocket::SessionDescriptionType::try_from(from.sdp_type)
            .map_err(|_| Self::Error::InvalidType(from.sdp_type))?;
        Ok(SessionDescription { sdp, sdp_type: (&sdp_type).into() })
    }
}

impl From<&websocket::SessionDescriptionType> for SdpType {
    fn from(from: &websocket::SessionDescriptionType) -> Self {
        match from {
            websocket::SessionDescriptionType::Answer => Self::Answer,
            websocket::SessionDescriptionType::Offer => Self::Offer,
            websocket::SessionDescriptionType::Pranswer => Self::Pranswer,
            websocket::SessionDescriptionType::Rollback => Self::Rollback,
        }
    }
}

impl From<&SdpType> for websocket::SessionDescriptionType {
    fn from(from: &SdpType) -> Self {
        match from {
            SdpType::Answer => Self::Answer,
            SdpType::Offer => Self::Offer,
            SdpType::Pranswer => Self::Pranswer,
            SdpType::Rollback => Self::Rollback,
        }
    }
}

impl From<websocket::IceCandidate> for IceCandidate {
    fn from(from: websocket::IceCandidate) -> Self {
        Self { candidate: from.candidate, mid: from.mid }
    }
}

impl From<IceCandidate> for websocket::IceCandidate {
    fn from(from: IceCandidate) -> Self {
        Self { candidate: from.candidate, mid: from.mid }
    }
}
