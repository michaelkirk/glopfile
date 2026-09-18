use futures::Future;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use wasm_bindgen_futures::spawn_local as spawn_impl;
    } else {
        use tokio::task::spawn_local as spawn_impl;
    }
}

pub fn spawn_local(future: impl Future<Output = ()> + 'static) {
    // the spawned task is detached on purpose
    let _detached = spawn_impl(future);
}
