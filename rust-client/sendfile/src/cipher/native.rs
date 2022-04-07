use std::panic::resume_unwind;
use std::sync::Arc;

use aes_gcm::aead::NewAead;
use aes_gcm::{AeadInPlace, Aes256Gcm};
use bytes::Bytes;
use tokio::task::spawn_blocking;

use crate::{Error, Result};

use super::buffer::{ContentCipherBufferCiphertextPartsMut, ContentCipherBufferPartsMut};
use super::{ContentCipherBuffer, KEY_SIZE};

pub struct NativeCipher {
    cipher: Arc<Aes256Gcm>,
}

#[async_trait::async_trait(?Send)]
impl super::Cipher for NativeCipher {
    async fn new(key: &[u8; KEY_SIZE]) -> Self {
        Self { cipher: Arc::new(Aes256Gcm::new(key.into())) }
    }

    async fn encrypt(&self, mut plaintext_and_nonce: ContentCipherBuffer, aad: Vec<u8>) -> Vec<u8> {
        // TODO: handle invalid crypt
        let cipher = Arc::clone(&self.cipher);
        spawn_blocking(move || {
            let ContentCipherBufferPartsMut { nonce, mut data } = plaintext_and_nonce.parts_mut();
            let payload = data.plaintext_mut();

            let new_tag = cipher
                .encrypt_in_place_detached((&*nonce).into(), &aad, payload)
                .expect("encryption failure");
            *data.ciphertext_parts_mut().tag = new_tag.into();
            plaintext_and_nonce.into_nonce_and_ciphertext()
        })
        .await
        .unwrap_or_else(|panic| resume_unwind(panic.into_panic()))
    }

    async fn decrypt(
        &self,
        mut ciphertext_and_nonce: ContentCipherBuffer,
        aad: Vec<u8>,
    ) -> Result<Bytes> {
        let cipher = Arc::clone(&self.cipher);
        spawn_blocking(move || {
            let ContentCipherBufferPartsMut { nonce, mut data } = ciphertext_and_nonce.parts_mut();
            let ContentCipherBufferCiphertextPartsMut { payload, tag } =
                data.ciphertext_parts_mut();
            cipher
                .decrypt_in_place_detached((&*nonce).into(), &aad, payload, (&*tag).into())
                .map_err(|_| Error::Decrypt)?;
            Ok(ciphertext_and_nonce.into_plaintext())
        })
        .await
        .unwrap_or_else(|panic| resume_unwind(panic.into_panic()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[tokio::test]
    async fn roundtrip() {
        let cipher_key = CipherKey::random();
        let cipher = ContentCipher::new(&cipher_key);
        let plaintext = b"Hello World";
        let ciphertext = cipher.encrypt(ContentCipherBuffer::from_plaintext(plaintext)).await;
        assert_eq!(
            plaintext.to_vec(),
            cipher.decrypt(ciphertext.into()).await.unwrap()
        );
    }

    #[tokio::test]
    async fn bad_crypt() {
        let cipher_key = CipherKey::random();
        let cipher = ContentCipher::new(&cipher_key);
        let plaintext = b"Hello World";
        let mut ciphertext = cipher.encrypt(ContentCipherBuffer::from_plaintext(plaintext)).await;
        ciphertext[0] = ciphertext[0] + 1;
        assert!(cipher.decrypt(ciphertext.into()).await.is_err());
    }
}
