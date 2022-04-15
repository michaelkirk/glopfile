use futures::StreamExt;
pub use tokio::sync::mpsc::UnboundedSender as Sender;
pub use tokio_stream::wrappers::UnboundedReceiverStream as Receiver;

use super::{RecvError, SendError};

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Receiver::new(rx))
}

impl<T> super::ChannelSend<T> for Sender<T> {
    fn send(&self, message: T) -> Result<(), SendError> {
        Sender::send(self, message).map_err(|_| SendError)
    }
}

#[async_trait::async_trait(?Send)]
impl<T: Send> super::ChannelReceive<T> for Receiver<T> {
    async fn recv(&mut self) -> Result<T, RecvError> {
        let result = self.next().await;
        result.ok_or(RecvError)
    }
}
