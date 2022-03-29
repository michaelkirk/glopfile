use aes_gcm::aead::{Aead, NewAead, Payload};
use aes_gcm::Aes256Gcm;

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

    async fn encrypt(&self, nonce: &Aes256GcmNonce, input: Payload<'_, '_>) -> Vec<u8> {
        // TODO: handle invalid crypt
        self.cipher.encrypt(nonce, input).expect("encryption failure")
    }

    async fn decrypt(&self, nonce: &Aes256GcmNonce, ciphertext: Payload<'_, '_>) -> Result<Vec<u8>> {
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| Error::InvalidInput("decryption error"))
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
        let ciphertext = cipher.encrypt(plaintext).await;
        assert_eq!(
            plaintext.to_vec(),
            cipher.decrypt(&ciphertext).await.unwrap()
        );
    }

    #[tokio::test]
    async fn bad_crypt() {
        let cipher_key = CipherKey::random();
        let cipher = ContentCipher::new(&cipher_key);
        let plaintext = b"Hello World";
        let mut ciphertext = cipher.encrypt(plaintext).await;
        ciphertext[0] = ciphertext[0] + 1;
        assert!(cipher.decrypt(&ciphertext).await.is_err());
    }
}
