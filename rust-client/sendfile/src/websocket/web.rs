use std::cell::Cell;
use std::ops::ControlFlow;
use std::rc::Rc;

use bytes::Bytes;
use js_sys::ArrayBuffer;
use js_sys::Promise;
use js_sys::Uint8Array;
use prost::Message;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::Event;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::util::web::Callbacks;

use super::protocol::*;
use super::WebSocketError;

pub struct WebWebSocketConnection {
    #[allow(unused)] // callbacks are held to maintain their reference counts
    callbacks: Callbacks,
    shared: Rc<Shared>,
}

struct Shared {
    websocket: WebSocket,
    error: Cell<Option<WebSocketError>>,
    closed: Cell<bool>,
}

#[async_trait::async_trait(?Send)]
impl super::WebSocketConnection for WebWebSocketConnection {
    async fn connect(
        url: &str,
        mut handle_incoming_message: impl FnMut(WebSocketMessage) -> ControlFlow<()> + 'static,
    ) -> Result<Self, WebSocketError> {
        let websocket = WebSocket::new(url)?;
        websocket.set_binary_type(BinaryType::Arraybuffer);

        let mut init_callbacks = Callbacks::default();

        if websocket.ready_state() == 0 {
            let open_promise = Promise::new({
                let websocket = websocket.clone();
                let init_callbacks = &mut init_callbacks;
                &mut move |resolve, reject| {
                    init_callbacks.add_event(
                        websocket.clone(),
                        WebSocket::set_onopen,
                        move |_event: Event| {
                            info!("websocket opened");
                            resolve
                                .call0(&JsValue::NULL)
                                .expect("Promise.resolve does not throw");
                        },
                    );

                    init_callbacks.add_event(
                        websocket.clone(),
                        WebSocket::set_onerror,
                        move |event: ErrorEvent| {
                            let error = event.error();
                            info!("error connecting to websocket: {error:?}");
                            reject
                                .call1(&JsValue::NULL, &error)
                                .expect("Promise.reject does not throw");
                        },
                    );
                }
            });
            JsFuture::from(open_promise).await?;
        }

        let mut callbacks = Callbacks::default();
        let shared = Rc::new(Shared {
            websocket: websocket.clone(),
            error: Default::default(),
            closed: Default::default(),
        });

        websocket.set_onopen(None);

        callbacks.add_event(websocket.clone(), WebSocket::set_onmessage, {
            let shared = Rc::clone(&shared);
            move |event: MessageEvent| {
                shared.catch(|| {
                    let data_buffer = event
                        .data()
                        .dyn_into::<ArrayBuffer>()
                        .expect("WebSocket message data is of type ArrayBuffer");
                    let data = Uint8Array::new(&data_buffer);
                    let message = WebSocketMessage::decode(Bytes::from(data.to_vec()))?;
                    debug!("received websocket message: {message:?}");
                    if let ControlFlow::Break(()) = handle_incoming_message(message) {
                        shared.close()?;
                    }
                    Ok(())
                })
            }
        });

        callbacks.add_event(websocket.clone(), WebSocket::set_onclose, {
            let shared = Rc::clone(&shared);
            move |event: CloseEvent| {
                info!(
                    "websocket closed with code {code}: {reason}",
                    code = event.code(),
                    reason = event.reason()
                );
                shared.closed.set(true);
            }
        });

        callbacks.add_event(websocket.clone(), WebSocket::set_onerror, {
            let shared = Rc::clone(&shared);
            move |event: ErrorEvent| shared.catch(|| Err(event.error())?)
        });

        drop(init_callbacks);

        if websocket.ready_state() == 1 {
            Ok(Self { callbacks, shared })
        } else {
            Err(WebSocketError::Closed)
        }
    }

    async fn send(&mut self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        self.shared.error.take().map(Err).unwrap_or(Ok(()))?;
        if self.shared.closed.get() {
            return Err(WebSocketError::Closed);
        }

        let encoded = message.encode_to_vec();
        self.shared.websocket.send_with_u8_array(&encoded)?;
        Ok(())
    }
}

impl Shared {
    fn catch(&self, fun: impl FnOnce() -> Result<(), WebSocketError>) {
        if let Err(error) = fun() {
            self.error.set(Some(error));
            self.closed.set(true);
        }
    }

    fn close(&self) -> Result<(), WebSocketError> {
        if !self.closed.get() {
            self.websocket.close()?;
            self.closed.set(true);
        }
        Ok(())
    }
}

impl From<JsValue> for WebSocketError {
    fn from(from: JsValue) -> Self {
        Self::WebSocketClient {
            source: from
                .as_string()
                .unwrap_or_else(|| format!("{from:?}"))
                .into(),
        }
    }
}
