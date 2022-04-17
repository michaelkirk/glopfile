use std::cell::Cell;
use std::ops::ControlFlow;
use std::rc::Rc;

use bytes::Bytes;
use futures::future::{abortable, pending, AbortHandle, Abortable, Aborted, Pending};
use futures::never::Never;
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
use super::{WebSocketError, WebSocketKnownCloseStatus};

pub struct WebWebSocketConnection {
    #[allow(unused)] // callbacks are held to maintain their reference counts
    callbacks: Callbacks,
    shared: Rc<Shared>,
}

struct Shared {
    websocket: WebSocket,
    error: Cell<Option<WebSocketError>>,
    closed: Cell<bool>,
    joiner_handle: AbortHandle,
    joiner: Abortable<Pending<Never>>,
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
        let (joiner, joiner_handle) = abortable(pending());
        let shared = Rc::new(Shared {
            websocket: websocket.clone(),
            error: Default::default(),
            closed: Default::default(),
            joiner_handle,
            joiner,
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
                let status = event.code().into();
                let reason = event.reason();
                shared
                    .error
                    .set(Some(WebSocketError::Closed { status, reason }));
                shared.set_closed();
            }
        });

        callbacks.add_event(websocket.clone(), WebSocket::set_onerror, {
            let shared = Rc::clone(&shared);
            move |event: ErrorEvent| shared.catch(|| Err(event.error().into()))
        });

        drop(init_callbacks);

        if websocket.ready_state() == 1 {
            Ok(Self { callbacks, shared })
        } else {
            shared.try_join().unwrap_or_else(|| {
                Err(WebSocketError::Closed {
                    status: WebSocketKnownCloseStatus::ConnectionClosed.into(),
                    reason: "closed immediately".into(),
                })
            })
        }
    }

    async fn send(&self, message: &WebSocketMessage) -> Result<(), WebSocketError> {
        if let Some(result) = self.shared.try_join() {
            return result;
        }

        let encoded = message.encode_to_vec();
        self.shared.websocket.send_with_u8_array(&encoded)?;
        Ok(())
    }

    async fn join(&self) -> Result<(), WebSocketError> {
        self.shared.join().await
    }
}

impl Shared {
    fn catch(&self, fun: impl FnOnce() -> Result<(), WebSocketError>) {
        if let Err(error) = fun() {
            self.error.set(Some(error));
            self.set_closed();
        }
    }

    fn close(&self) -> Result<(), WebSocketError> {
        if !self.closed.get() {
            self.websocket.close()?;
            self.set_closed();
        }
        Ok(())
    }

    fn set_closed(&self) {
        self.closed.set(true);
        self.joiner_handle.abort();
    }

    fn try_join<T>(&self) -> Option<Result<T, WebSocketError>> {
        if let Some(error) = self.error.take() {
            Some(Err(error))
        } else if self.closed.get() {
            Some(Err(WebSocketError::Closed {
                status: WebSocketKnownCloseStatus::ConnectionClosed.into(),
                reason: "closed by application".into(),
            }))
        } else {
            None
        }
    }

    async fn join(&self) -> Result<(), WebSocketError> {
        let joiner = self.joiner.clone();
        match joiner.await {
            Ok(never) => match never {},
            Err(Aborted) => self.try_join().unwrap(),
        }
    }
}

impl From<JsValue> for WebSocketError {
    fn from(from: JsValue) -> Self {
        // TODO Sadly we cannot tell what type of error this is, whether it be an HTTP Client Error, Server Error, or
        // plain IO Error. This matters because some errors are retriable and some are not. Maybe we could try to probe
        // the server by sending a non-websocket HTTP request to find out the status code?
        Self::WebSocketClient {
            source: from
                .as_string()
                .unwrap_or_else(|| format!("{from:?}"))
                .into(),
        }
    }
}
