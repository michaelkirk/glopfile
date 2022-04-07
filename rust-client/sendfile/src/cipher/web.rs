use bytes::Bytes;
use js_sys::{Array, JsString, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, AesGcmParams, CryptoKey, SubtleCrypto};

use crate::{Error, Result};

use super::{buffer::ContentCipherBufferPartsMut, ContentCipherBuffer, KEY_SIZE};

pub struct WebCipher {
    subtle: SubtleCrypto,
    key: CryptoKey,
}

#[async_trait::async_trait(?Send)]
impl super::Cipher for WebCipher {
    async fn new(key: &[u8; KEY_SIZE]) -> Self
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

    async fn encrypt(&self, mut plaintext_and_nonce: ContentCipherBuffer, aad: Vec<u8>) -> Vec<u8> {
        let ContentCipherBufferPartsMut { nonce, mut data } = plaintext_and_nonce.parts_mut();
        let plaintext = data.plaintext_mut();

        let nonce = Uint8Array::from(&nonce[..]);
        let aad = Uint8Array::from(&aad[..]);

        let mut params = AesGcmParams::new("AES-GCM", &nonce);
        params.additional_data(&aad);
        let encryption_promise = self
            .subtle
            .encrypt_with_object_and_u8_array(&params, &self.key, plaintext)
            .expect("provided valid parameters to SubtleCrypto.encrypt");
        let output = JsFuture::from(encryption_promise)
            .await
            .expect("SubtleCrypto.encrypt promise returns a value");

        let ciphertext = data.ciphertext_mut();
        Uint8Array::new(&output).copy_to(ciphertext);
        plaintext_and_nonce.into_nonce_and_ciphertext()
    }

    async fn decrypt(
        &self,
        mut ciphertext_and_nonce: ContentCipherBuffer,
        aad: Vec<u8>,
    ) -> Result<Bytes> {
        let ContentCipherBufferPartsMut { nonce, mut data } = ciphertext_and_nonce.parts_mut();
        let ciphertext = data.ciphertext_mut();

        let nonce = Uint8Array::from(&nonce[..]);
        let aad = Uint8Array::from(&aad[..]);

        let mut params = AesGcmParams::new("AES-GCM", &nonce);
        params.additional_data(&aad);
        let encryption_promise = self
            .subtle
            .decrypt_with_object_and_u8_array(&params, &self.key, ciphertext)
            .expect("provided valid parameters to SubtleCrypto.encrypt");
        let output = JsFuture::from(encryption_promise)
            .await
            .map_err(|_| Error::Decrypt)?;

        let plaintext = data.plaintext_mut();
        Uint8Array::new(&output).copy_to(plaintext);
        Ok(ciphertext_and_nonce.into_plaintext())
    }
}
