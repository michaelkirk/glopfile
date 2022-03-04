pub use tokio::sync::mpsc::unbounded_channel as channel;
pub use tokio::sync::mpsc::UnboundedReceiver as Receiver;
pub use tokio::sync::mpsc::UnboundedSender as Sender;

use super::{RecvError, RecvTimeoutError, SendError};

impl<T> super::ChannelSend<T> for Sender<T> {
    fn send(&self, message: T) -> Result<(), SendError> {
        Sender::send(self, message).map_err(|_| SendError)
    }
}

#[async_trait::async_trait(?Send)]
impl<T: Send> super::ChannelReceive<T> for Receiver<T> {
    async fn recv(&mut self) -> Result<T, RecvError> {
        let result = Receiver::recv(self).await;
        result.ok_or(RecvError)
    }

    async fn recv_timeout(&mut self, timeout: instant::Duration) -> Result<T, RecvTimeoutError> {
        let result = tokio::time::timeout(timeout.into(), self.recv()).await;
        let result = result.map_err(|tokio::time::error::Elapsed { .. }| RecvTimeoutError::Timeout)?;
        result.ok_or(RecvTimeoutError::Disconnected)
    }
}
