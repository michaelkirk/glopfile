use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::channel::oneshot;
use futures::ready;
use futures::{Future, TryFutureExt};
use js_sys::{Function, Promise};
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

pub(crate) fn return_promise<F, T>(future: F) -> Promise
where
    F: Future<Output = Result<T, js_sys::Error>> + 'static,
    T: Into<JsValue>,
{
    let future = future
        .map_ok(|output| output.into())
        .map_err(|error| error.into());
    wasm_bindgen_futures::future_to_promise(future)
}

pub(crate) use gloo_timers::future::{sleep, TimeoutFuture as Sleep};

pub(crate) fn send_sleep(duration: Duration) -> SendSleep {
    let (tx, rx) = oneshot::channel();
    spawn_local(async move {
        sleep(duration).await;
        let _ignore = tx.send(());
    });
    SendSleep(rx)
}

#[pin_project::pin_project]
pub(crate) struct SendSleep(#[pin] oneshot::Receiver<()>);

impl Future for SendSleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        ready!(self.project().0.poll(cx)).expect("sleep task died");
        Poll::Ready(())
    }
}

#[derive(Default)]
pub(crate) struct Callbacks {
    callbacks: Vec<Callback>,
}

struct Callback {
    #[allow(unused)] // closures are held to maintain their reference counts
    closure: Box<dyn AsRef<JsValue> + 'static>,
    cleanup: Option<Box<dyn FnOnce() + 'static>>,
}

impl Callbacks {
    pub fn add_event<ReceiverTy, AddFnTy, F, E>(
        &mut self,
        receiver: ReceiverTy,
        mut add_fun: AddFnTy,
        fun: F,
    ) where
        ReceiverTy: 'static,
        AddFnTy: FnMut(&ReceiverTy, Option<&Function>) + 'static,
        F: FnMut(E) + 'static,
        E: FromWasmAbi + 'static,
    {
        let closure = Closure::wrap(Box::new(fun) as Box<dyn FnMut(E)>);
        add_fun(
            &receiver,
            Some(closure.as_ref().unchecked_ref::<Function>()),
        );
        self.callbacks.push(Callback {
            closure: Box::new(closure),
            cleanup: Some(Box::new(move || add_fun(&receiver, None))),
        });
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}
