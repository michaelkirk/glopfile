use js_sys::Function;
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[derive(Default)]
pub(crate) struct Callbacks {
    callbacks: Vec<Callback>,
}

struct Callback {
    #[allow(unused)] // closures are held to maintain their reference counts
    closure: Box<dyn AsRef<JsValue> + 'static>,
    cleanup: Option<Box<dyn FnOnce() + 'static>>,
}

impl Callbacks {
    pub fn add_event<ReceiverTy, AddFnTy, F, E>(
        &mut self,
        receiver: ReceiverTy,
        mut add_fun: AddFnTy,
        fun: F,
    ) where
        ReceiverTy: 'static,
        AddFnTy: FnMut(&ReceiverTy, Option<&Function>) + 'static,
        F: FnMut(E) + 'static,
        E: FromWasmAbi + 'static,
    {
        let closure = Closure::new(fun);
        add_fun(
            &receiver,
            Some(closure.as_ref().unchecked_ref::<Function>()),
        );
        self.callbacks.push(Callback {
            closure: Box::new(closure),
            cleanup: Some(Box::new(move || add_fun(&receiver, None))),
        });
    }
}

impl Drop for Callback {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}
