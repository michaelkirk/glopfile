use js_sys::{Array, ArrayBuffer, JsString, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::RtcDataChannelType;
use web_sys::{
    ErrorEvent, Event, MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent,
    RtcDataChannelInit, RtcIceCandidate, RtcIceCandidateInit, RtcIceServer, RtcPeerConnection,
    RtcPeerConnectionIceEvent, RtcSdpType, RtcSessionDescription, RtcSessionDescriptionInit,
};

use crate::p2p::PeerConnectionEvent;
use crate::util::web::Callbacks;
use crate::{websocket, Error};

use super::PeerConnectionEventHandler;

pub enum WebRtc {}

pub struct WebRtcPeerConnection {
    rtc: RtcPeerConnection,
    tx: PeerConnectionEventHandler,
    #[allow(unused)] // callbacks are held to maintain their reference counts
    callbacks: Callbacks,
}

pub struct WebRtcDataChannel {
    rtc: RtcDataChannel,
    #[allow(unused)] // callbacks are held to maintain their reference counts
    callbacks: Callbacks,
}

#[derive(Debug, thiserror::Error)]
#[error("WebRTC error: {0}")]
pub(crate) struct WebRtcError(String);

impl WebRtcPeerConnection {
    async fn send_local_description(
        &self,
        local_description_init: &RtcSessionDescriptionInit,
    ) -> Result<(), Error> {
        JsFuture::from(self.rtc.set_local_description(local_description_init))
            .await
            .map_err(WebRtcError::from)?;
        let local_description = self.rtc.local_description().unwrap();

        self.tx
            .handle(PeerConnectionEvent::OutgoingSignalingMessage(
                websocket::rtc_signaling_message::Inner::SessionDescription(
                    (&local_description).into(),
                ),
            ));

        Ok(())
    }
}

impl super::Rtc for WebRtc {
    type PeerConnection = WebRtcPeerConnection;

    fn new_peer_connection(
        stun_servers: &[&str],
        tx: PeerConnectionEventHandler,
    ) -> Result<Self::PeerConnection, Error> {
        let mut callbacks = Callbacks::default();

        let stun_servers = stun_servers.iter().map(|url| {
            let mut stun_server = RtcIceServer::new();
            stun_server.urls(&JsString::from(*url));
            stun_server
        });

        let mut rtc_config = RtcConfiguration::new();
        rtc_config.ice_servers(&stun_servers.collect::<Array>());

        let rtc =
            RtcPeerConnection::new_with_configuration(&rtc_config).map_err(WebRtcError::from)?;

        callbacks.add_event(rtc.clone(), RtcPeerConnection::set_onicecandidate, {
            let tx = tx.clone();
            move |ev: RtcPeerConnectionIceEvent| {
                if let Some(candidate) = ev.candidate() {
                    tx.handle(PeerConnectionEvent::OutgoingSignalingMessage(
                        websocket::rtc_signaling_message::Inner::IceCandidate((&candidate).into()),
                    ));
                }
            }
        });

        callbacks.add_event(
            rtc.clone(),
            RtcPeerConnection::set_oniceconnectionstatechange,
            {
                let peer_connection = rtc.clone();
                move |_event: Event| {
                    info!(
                        "RTC connection state changed: {state:?}",
                        state = peer_connection.ice_connection_state()
                    );
                }
            },
        );

        callbacks.add_event(
            rtc.clone(),
            RtcPeerConnection::set_onicegatheringstatechange,
            {
                let peer_connection = rtc.clone();
                move |_event: Event| {
                    info!(
                        "RTC candidate gathering state changed: {state:?}",
                        state = peer_connection.ice_gathering_state(),
                    );
                }
            },
        );

        callbacks.add_event(
            rtc.clone(),
            RtcPeerConnection::set_onsignalingstatechange,
            {
                let peer_connection = rtc.clone();
                move |_event: Event| {
                    info!(
                        "RTC signaling state changed: {state:?}",
                        state = peer_connection.signaling_state(),
                    );
                }
            },
        );

        callbacks.add_event(rtc.clone(), RtcPeerConnection::set_ondatachannel, {
            move |event: RtcDataChannelEvent| {
                let channel = event.channel();
                panic!(
                    "unexpected RTC data channel stream received: {label}",
                    label = channel.label(),
                );
            }
        });

        Ok(WebRtcPeerConnection { rtc, tx, callbacks })
    }
}

#[async_trait::async_trait(?Send)]
impl super::RtcPeerConnection for WebRtcPeerConnection {
    type DataChannel = WebRtcDataChannel;

    fn new_data_channel(
        &mut self,
        id: u16,
        label: &str,
        tx: PeerConnectionEventHandler,
    ) -> Result<Self::DataChannel, Error> {
        let mut callbacks = Callbacks::default();

        let mut init = RtcDataChannelInit::new();
        init.negotiated(true);
        init.id(id);

        let rtc = self
            .rtc
            .create_data_channel_with_data_channel_dict(label, &init);
        rtc.set_binary_type(RtcDataChannelType::Arraybuffer);

        callbacks.add_event(rtc.clone(), RtcDataChannel::set_onopen, {
            let tx = tx.clone();
            move |_event: Event| {
                tx.handle(PeerConnectionEvent::DataChannelOpened);
            }
        });

        callbacks.add_event(
            rtc.clone(),
            RtcDataChannel::set_onclose,
            move |_event: Event| {
                info!("RTC data channel closed");
            },
        );

        callbacks.add_event(rtc.clone(), RtcDataChannel::set_onerror, {
            let tx = tx.clone();
            move |event: ErrorEvent| {
                let error = WebRtcError::from(event.error());
                tx.handle(PeerConnectionEvent::DataChannelError(Box::new(error)));
            }
        });

        callbacks.add_event(rtc.clone(), RtcDataChannel::set_onmessage, {
            move |event: MessageEvent| {
                let msg_buffer = event
                    .data()
                    .dyn_into::<ArrayBuffer>()
                    .expect("RTC message data is of type ArrayBuffer");
                let len = msg_buffer.byte_length();

                trace!("RTC data channel message received of len {len}");

                let msg = Uint8Array::new(msg_buffer.as_ref()).to_vec();

                tx.handle(PeerConnectionEvent::DataChannelMessage(msg.into()));
            }
        });

        callbacks.add_event(
            rtc.clone(),
            RtcDataChannel::set_onbufferedamountlow,
            move |_event: Event| {
                trace!("RTC data channel buffer now empty");
            },
        );

        Ok(Self::DataChannel { rtc, callbacks })
    }

    async fn create_offer(&mut self) -> Result<(), Error> {
        let local_description = JsFuture::from(self.rtc.create_offer())
            .await
            .map_err(WebRtcError::from)?;
        // NB: we must use unchecked_ref here since RtcSessionDescriptionInit is not a real type in js
        let local_description = local_description.unchecked_ref();
        self.send_local_description(local_description).await?;
        Ok(())
    }

    async fn create_answer(&mut self) -> Result<(), Error> {
        let local_description = JsFuture::from(self.rtc.create_answer())
            .await
            .map_err(WebRtcError::from)?;
        // NB: we must use unchecked_ref here since RtcSessionDescriptionInit is not a real type in js
        let local_description = local_description.unchecked_ref();
        self.send_local_description(local_description).await?;
        Ok(())
    }

    async fn set_remote_description(
        &mut self,
        description: websocket::SessionDescription,
    ) -> Result<(), Error> {
        JsFuture::from(self.rtc.set_remote_description(&(&description).into()))
            .await
            .map_err(WebRtcError::from)?;
        Ok(())
    }

    fn local_description_type(&self) -> Option<websocket::SessionDescriptionType> {
        self.rtc
            .local_description()
            .map(|description| (&description.type_()).into())
    }

    async fn add_remote_candidate(
        &mut self,
        candidate: websocket::IceCandidate,
    ) -> Result<(), Error> {
        let add_ice_candidate_promise = self
            .rtc
            .add_ice_candidate_with_opt_rtc_ice_candidate(Some(&(&candidate).into()));
        JsFuture::from(add_ice_candidate_promise)
            .await
            .map_err(WebRtcError::from)?;
        Ok(())
    }
}

impl super::RtcDataChannel for WebRtcDataChannel {
    fn send(&mut self, message: &[u8]) -> Result<(), Error> {
        self.rtc
            .send_with_u8_array(message)
            .map_err(WebRtcError::from)?;
        Ok(())
    }
}

impl From<JsValue> for WebRtcError {
    fn from(from: JsValue) -> Self {
        Self(from.as_string().unwrap_or_else(|| format!("{from:?}")))
    }
}

impl From<WebRtcError> for Error {
    fn from(from: WebRtcError) -> Self {
        Error::rtc_err(from)
    }
}

impl From<&RtcIceCandidate> for websocket::IceCandidate {
    fn from(from: &RtcIceCandidate) -> Self {
        Self { candidate: from.candidate(), mid: from.sdp_mid().unwrap_or_default() }
    }
}

impl From<&websocket::IceCandidate> for RtcIceCandidate {
    fn from(from: &websocket::IceCandidate) -> Self {
        let mut init = RtcIceCandidateInit::new(&from.candidate);
        init.sdp_mid(Some(&from.mid));
        Self::new(&init).expect("RtcIceCandidate constructor used correctly")
    }
}

impl From<&RtcSessionDescription> for websocket::SessionDescription {
    fn from(from: &RtcSessionDescription) -> Self {
        Self {
            sdp: from.sdp(),
            sdp_type: websocket::SessionDescriptionType::from(&from.type_()).into(),
        }
    }
}

impl From<&websocket::SessionDescription> for RtcSessionDescriptionInit {
    fn from(from: &websocket::SessionDescription) -> Self {
        let mut init = Self::new((&from.sdp_type()).into());
        init.sdp(&from.sdp);
        init
    }
}

impl From<&RtcSdpType> for websocket::SessionDescriptionType {
    fn from(from: &RtcSdpType) -> Self {
        match from {
            RtcSdpType::Offer => Self::Offer,
            RtcSdpType::Answer => Self::Answer,
            RtcSdpType::Pranswer => Self::Pranswer,
            RtcSdpType::Rollback => Self::Rollback,
            _ => panic!("invalid WebRTC SDP type: {from:?}"),
        }
    }
}

impl From<&websocket::SessionDescriptionType> for RtcSdpType {
    fn from(from: &websocket::SessionDescriptionType) -> Self {
        match from {
            websocket::SessionDescriptionType::Offer => Self::Offer,
            websocket::SessionDescriptionType::Answer => Self::Answer,
            websocket::SessionDescriptionType::Pranswer => Self::Pranswer,
            websocket::SessionDescriptionType::Rollback => Self::Rollback,
        }
    }
}
