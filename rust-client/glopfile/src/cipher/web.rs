use bytes::Bytes;
use js_sys::{Array, JsString, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, AesGcmParams, AesKeyGenParams, CryptoKey, HkdfParams, SubtleCrypto};

use crate::{Error, Result};

use super::{buffer::ContentCipherBufferPartsMut, CipherKey, ContentCipherBuffer, KEY_SIZE};

pub struct WebCipher {
    subtle: SubtleCrypto,
    key: CryptoKey,
}

#[async_trait::async_trait(?Send)]
impl super::CipherImpl for WebCipher {
    async fn derive_new(base_key: &CipherKey, hkdf_info: &[u8]) -> Self
    where
        Self: Sized,
    {
        let window = window().expect("web Window context present");
        let crypto = window.crypto().expect("Window.crypto accessible");
        let subtle = crypto.subtle();

        let base_key_js = {
            let key_usages = ["deriveKey"]
                .into_iter()
                .map(JsString::from)
                .collect::<Array>();

            let key_result = subtle
                .import_key_with_str(
                    "raw",
                    &Uint8Array::from(&base_key.bytes()[..]),
                    "HKDF",
                    false,
                    &key_usages,
                )
                .expect("provided valid parameters to SubtleCrypto.importKey");

            JsFuture::from(key_result)
                .await
                .expect("SubtleCrypto.importKey promise returns a value")
                .dyn_into::<CryptoKey>()
                .expect("SubtleCrypto.importKey promise returns a CryptoKey")
        };

        let derived_key = {
            let salt = Uint8Array::new_with_length(0);
            let algorithm = HkdfParams::new(
                "HKDF",
                &JsString::from("SHA-256"),
                &Uint8Array::from(hkdf_info),
                &salt,
            );

            let key_usages = ["encrypt", "decrypt"]
                .into_iter()
                .map(JsString::from)
                .collect::<Array>();

            let key_size_bits = 8 * KEY_SIZE;
            let derived_key_type = AesKeyGenParams::new(
                "AES-GCM",
                key_size_bits.try_into().expect("invalid key size"),
            );

            let key_result = subtle
                .derive_key_with_object_and_object(
                    &algorithm,
                    &base_key_js,
                    &derived_key_type,
                    false,
                    &key_usages,
                )
                .expect("provided valid parameters to SubtleCrypto.deriveKey");

            JsFuture::from(key_result)
                .await
                .expect("SubtleCrypto.deriveKey promise returns a value")
                .dyn_into::<CryptoKey>()
                .expect("SubtleCrypto.deriveKey promise returns a CryptoKey")
        };

        Self { subtle, key: derived_key }
    }

    async fn encrypt(&self, mut plaintext_and_nonce: ContentCipherBuffer, aad: Vec<u8>) -> Vec<u8> {
        let ContentCipherBufferPartsMut { nonce, mut data } = plaintext_and_nonce.parts_mut();
        let plaintext = data.plaintext_mut();

        let nonce = Uint8Array::from(&nonce[..]);
        let aad = Uint8Array::from(&aad[..]);

        let params = AesGcmParams::new("AES-GCM", &nonce);
        params.set_additional_data(&aad);
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

        let params = AesGcmParams::new("AES-GCM", &nonce);
        params.set_additional_data(&aad);
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
