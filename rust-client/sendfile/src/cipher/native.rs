use std::panic::resume_unwind;

use aes_gcm::aead::{Aead, NewAead, Payload};
use aes_gcm::Aes256Gcm;
use bytes::Bytes;
use tokio::task::spawn_blocking;

use super::{Aes256GcmKey, Aes256GcmNonce};
use crate::{Error, Result};

pub struct NativeCipher {
    cipher: Aes256Gcm,
}

#[async_trait::async_trait(?Send)]
impl super::Cipher for NativeCipher {
    async fn new(key: &Aes256GcmKey) -> Self {
        Self { cipher: Aes256Gcm::new(key) }
    }

    async fn encrypt(&self, nonce: &Aes256GcmNonce, plaintext: Bytes, aad: Bytes) -> Vec<u8> {
        // TODO: handle invalid crypt
        let cipher = self.cipher.clone();
        let nonce = nonce.clone();
        spawn_blocking(move || {
            let payload = Payload { msg: &plaintext, aad: &aad };
            cipher.encrypt(&nonce, payload)
        })
        .await
        .unwrap_or_else(|panic| resume_unwind(panic.into_panic()))
        .expect("encryption failure")
    }

    async fn decrypt(
        &self,
        nonce: &Aes256GcmNonce,
        ciphertext: Bytes,
        aad: Bytes,
    ) -> Result<Vec<u8>> {
        let cipher = self.cipher.clone();
        let nonce = nonce.clone();
        spawn_blocking(move || {
            let payload = Payload { msg: &ciphertext, aad: &aad };
            cipher.decrypt(&nonce, payload).map_err(|_| Error::Decrypt)
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
        let ciphertext = cipher.encrypt(plaintext[..].into()).await;
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
        let mut ciphertext = cipher.encrypt(plaintext[..].into()).await;
        ciphertext[0] = ciphertext[0] + 1;
        assert!(cipher.decrypt(ciphertext.into()).await.is_err());
    }
}
