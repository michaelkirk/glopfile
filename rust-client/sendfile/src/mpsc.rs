use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use instant::Duration;

use crate::util::{timeout, TimeoutError};

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub use web::Sender as DefaultSender;
        pub use web::Receiver as DefaultReceiver;
        use web::channel as default_channel;
    } else {
        pub use native::Sender as DefaultSender;
        pub use native::Receiver as DefaultReceiver;
        use native::channel as default_channel;
    }
}

pub struct Sender<T, C = DefaultSender<T>> {
    tx: C,
    _t: PhantomData<fn() -> T>,
}

#[pin_project::pin_project]
pub struct Receiver<T, C = DefaultReceiver<T>> {
    #[pin]
    rx: C,
    _t: PhantomData<fn() -> T>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("mpsc channel closed by receiver")]
pub struct SendError;

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("mpsc channel closed by senders")]
pub struct RecvError;

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum RecvTimeoutError {
    #[error("mpsc channel closed by senders")]
    Disconnected,
    #[error("mpsc channel receive timed out")]
    Timeout,
}

pub trait ChannelSend<T> {
    fn send(&self, message: T) -> Result<(), SendError>;
}

#[async_trait::async_trait(?Send)]
pub trait ChannelReceive<T>: Stream<Item = T> {
    async fn recv(&mut self) -> Result<T, RecvError>;
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = default_channel();
    (
        Sender { tx, _t: PhantomData },
        Receiver { rx, _t: PhantomData },
    )
}

impl<T, C: ChannelSend<T>> Sender<T, C> {
    pub fn send(&self, message: T) -> Result<(), SendError> {
        self.tx.send(message)
    }
}

impl<T, C: Clone> Clone for Sender<T, C> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), _t: PhantomData }
    }
}

impl<T, C: ChannelReceive<T>> Receiver<T, C> {
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        self.rx.recv().await
    }

    pub async fn recv_timeout(&mut self, duration: Duration) -> Result<T, RecvTimeoutError> {
        timeout(duration, self.rx.recv())
            .await
            .map_err(|TimeoutError| RecvTimeoutError::Timeout)?
            .map_err(|RecvError| RecvTimeoutError::Disconnected)
    }
}

impl<T, C: ChannelReceive<T>> Stream for Receiver<T, C> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().rx.poll_next(cx)
    }
}
