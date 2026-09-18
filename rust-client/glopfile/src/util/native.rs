use futures::Future;
use tokio::task::LocalSet;

/// Await `future` within a [`LocalSet`] on a [tokio] [`Runtime`] on the current thread.
///
/// All tasks spawed by [`spawn_local`] will be dropped, instead of being driven to completion, when `future` completes.
///
/// [`Runtime`]: tokio::runtime::Runtime
/// [`spawn_local`]: tokio::task::spawn_local
pub fn current_thread_block_on<F, T, E>(future: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
    E: From<std::io::Error>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let local_set = LocalSet::new();
        local_set.run_until(future).await
    })
}
