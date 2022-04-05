use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{ready, Future, Stream};

use crate::mpsc;

#[pin_project::pin_project]
pub struct Progress<T, F, C = mpsc::DefaultReceiver<ProgressState<T>>> {
    #[pin]
    progress_rx: mpsc::Receiver<ProgressState<T>, C>,
    #[pin]
    future: F,
}

#[derive(Clone, Copy)]
pub struct ProgressState<T> {
    pub current: T,
    pub total: T,
}

impl<T, F, C> Progress<T, F, C> {
    pub fn new(progress_rx: mpsc::Receiver<ProgressState<T>, C>, future: F) -> Self {
        Self { future, progress_rx }
    }
}

impl<T, F> Progress<T, F, mpsc::DefaultReceiver<ProgressState<T>>> {
    pub fn new_with<Fun>(fun: Fun) -> Self
    where
        Fun: FnOnce(mpsc::Sender<ProgressState<T>>) -> F,
    {
        let (progress_tx, progress_rx) = mpsc::channel();
        let future = fun(progress_tx);
        Self::new(progress_rx, future)
    }
}

impl<T, F, E, C> Stream for Progress<T, F, C>
where
    F: Future<Output = Result<(), E>>,
    C: mpsc::ChannelReceive<ProgressState<T>>,
{
    type Item = Result<ProgressState<T>, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().poll(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(None),
            Poll::Ready(Err(error)) => Poll::Ready(Some(Err(error))),
            Poll::Pending => {
                let next = ready!(self.project().progress_rx.poll_next(cx));
                Poll::Ready(Ok(next).transpose())
            }
        }
    }
}

impl<T, F: Future, C> Future for Progress<T, F, C> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().future.poll(cx)
    }
}

impl<T: Into<f64>> ProgressState<T> {
    pub fn percent_complete(self) -> f64 {
        let (current, total): (f64, f64) = (self.current.into(), self.total.into());
        (current / total).clamp(0.0, 100.0)
    }
}
