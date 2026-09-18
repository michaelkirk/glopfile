use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::mpsc;
use futures::{ready, Future, Stream};

#[pin_project::pin_project]
pub struct Progress<T, F> {
    #[pin]
    progress_rx: mpsc::UnboundedReceiver<ProgressState<T>>,
    #[pin]
    future: F,
}

#[derive(Clone, Copy)]
pub struct ProgressState<T> {
    pub current: T,
    pub total: T,
}

impl<T, F> Progress<T, F> {
    pub fn new(progress_rx: mpsc::UnboundedReceiver<ProgressState<T>>, future: F) -> Self {
        Self { future, progress_rx }
    }
}

impl<T, F> Progress<T, F> {
    pub fn new_with<Fun>(fun: Fun) -> Self
    where
        Fun: FnOnce(mpsc::UnboundedSender<ProgressState<T>>) -> F,
    {
        let (progress_tx, progress_rx) = mpsc::unbounded();
        let future = fun(progress_tx);
        Self::new(progress_rx, future)
    }
}

impl<T, F, E> Stream for Progress<T, F>
where
    F: Future<Output = Result<(), E>>,
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

impl<T, F: Future> Future for Progress<T, F> {
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
