use aes_gcm::aead::Payload;
use js_sys::{Array, JsString, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, AesGcmParams, CryptoKey, SubtleCrypto};

use super::{Aes256GcmKey, Aes256GcmNonce};
use crate::{Error, Result};

pub struct WebCipher {
    subtle: SubtleCrypto,
    key: CryptoKey,
}

#[async_trait::async_trait(?Send)]
impl super::Cipher for WebCipher {
    async fn new(key: &Aes256GcmKey) -> Self
    where
        Self: Sized,
    {
        let window = window().expect("web Window context present");
        let crypto = window.crypto().expect("Window.crypto accessible");
        let subtle = crypto.subtle();

        let key_usages = ["encrypt", "decrypt"];
        let key_usages = key_usages
            .into_iter()
            .map(JsString::from)
            .collect::<Array>();

        let key_result = subtle.import_key_with_str(
            "raw",
            &Uint8Array::from(&key[..]),
            "AES-GCM",
            false,
            &key_usages,
        );
        let key_future = JsFuture::from(
            key_result.expect("provided valid parameters to SubtleCrypto.importKey"),
        );

        let key = key_future
            .await
            .expect("SubtleCrypto.importKey promise returns a value")
            .dyn_into::<CryptoKey>()
            .expect("SubtleCrypto.importKey promise returns a CryptoKey");

        Self { subtle, key }
    }

    async fn encrypt(&self, nonce: &Aes256GcmNonce, plaintext: Payload<'_, '_>) -> Vec<u8> {
        let iv = Uint8Array::from(&nonce[..]);
        let aad = Uint8Array::from(plaintext.aad);
        let data = Uint8Array::from(plaintext.msg);

        let mut params = AesGcmParams::new("AES-GCM", &iv);
        params.additional_data(&aad);
        let encryption_promise = self
            .subtle
            .encrypt_with_object_and_buffer_source(&params, &self.key, &data)
            .expect("provided valid parameters to SubtleCrypto.encrypt");
        let output = JsFuture::from(encryption_promise)
            .await
            .expect("SubtleCrypto.encrypt promise returns a value");
        Uint8Array::new(&output).to_vec()
    }

    async fn decrypt(
        &self,
        nonce: &Aes256GcmNonce,
        ciphertext: Payload<'_, '_>,
    ) -> Result<Vec<u8>> {
        let iv = Uint8Array::from(&nonce[..]);
        let aad = Uint8Array::from(ciphertext.aad);
        let data = Uint8Array::from(ciphertext.msg);

        let mut params = AesGcmParams::new("AES-GCM", &iv);
        params.additional_data(&aad);
        let encryption_promise = self
            .subtle
            .decrypt_with_object_and_buffer_source(&params, &self.key, &data)
            .expect("provided valid parameters to SubtleCrypto.encrypt");
        let output = JsFuture::from(encryption_promise)
            .await
            .map_err(|_| Error::InvalidInput("decryption error"))?;
        Ok(Uint8Array::new(&output).to_vec())
    }
}
