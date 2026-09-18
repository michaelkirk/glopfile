use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::channel::oneshot;
use futures::{ready, Future};
use wasm_bindgen_futures::spawn_local;

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
