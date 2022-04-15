use futures::channel::mpsc::TrySendError;
use futures::StreamExt;

pub use futures::channel::mpsc::unbounded as channel;
pub use futures::channel::mpsc::UnboundedReceiver as Receiver;
pub use futures::channel::mpsc::UnboundedSender as Sender;

impl<T> super::ChannelSend<T> for Sender<T> {
    fn send(&self, message: T) -> Result<(), super::SendError> {
        Sender::unbounded_send(self, message).map_err(|TrySendError { .. }| super::SendError)
    }
}

#[async_trait::async_trait(?Send)]
impl<T: Send> super::ChannelReceive<T> for Receiver<T> {
    async fn recv(&mut self) -> Result<T, super::RecvError> {
        let result = self.next().await;
        result.ok_or(super::RecvError)
    }
}
