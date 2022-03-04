use futures::channel::mpsc::TrySendError;
use futures::FutureExt;
use futures::StreamExt;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;
use web_sys::window;

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

    async fn recv_timeout(
        &mut self,
        timeout: instant::Duration,
    ) -> Result<T, super::RecvTimeoutError> {
        let mut timeout_handle = None;
        let timeout_promise = Promise::new(&mut |resolve, _reject| {
            let timeout_millis = timeout
                .as_millis()
                .try_into()
                .expect("timeout ms less than 32 bits long");
            let window = window().unwrap();
            let ok_timeout_handle = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, timeout_millis)
                .unwrap();
            timeout_handle = Some(ok_timeout_handle);
        });
        let mut recv_future = self.recv().fuse();
        let mut timeout_future = JsFuture::from(timeout_promise).fuse();
        futures::select! {
            _ = timeout_future => Err(super::RecvTimeoutError::Timeout),
            recv_result = recv_future => recv_result.map_err(|super::RecvError| super::RecvTimeoutError::Disconnected)
        }
    }
}
