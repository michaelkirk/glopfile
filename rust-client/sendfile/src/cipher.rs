use std::convert::TryInto;

use aes_gcm::aead::{Aead, NewAead};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use zeroize::ZeroizeOnDrop;

use crate::url_safe_base64;
use crate::{Error, Result};

#[derive(ZeroizeOnDrop, Clone)]
pub struct CipherKey {
    bytes: [u8; 32],
}

impl std::fmt::Debug for CipherKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("CipherKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl CipherKey {
    pub fn random() -> Self {
        Self {
            bytes: rand::random(),
        }
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn from_string(url_safe_base64: &str) -> Result<Self> {
        let bytes = url_safe_base64::decode(url_safe_base64)
            .map_err(|_| Error::InvalidInput("invalid base64 encoding for cipher key"))?;

        let byte_array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::InvalidInput("invalid byte length for cipher key"))?;

        Ok(Self::from_bytes(byte_array))
    }

    pub fn serialized(&self) -> String {
        url_safe_base64::encode(self.bytes())
    }
}

pub(crate) struct ContentCipher<'a> {
    cipher_key: &'a CipherKey,
}
impl<'a> ContentCipher<'a> {
    pub fn new(cipher_key: &'a CipherKey) -> Self {
        Self { cipher_key }
    }

    fn cipher(&self) -> Aes256Gcm {
        let key = Key::from_slice(self.cipher_key.bytes());
        Aes256Gcm::new(key)
    }

    // TODO: stream
    pub fn encrypt(&self, input: &[u8]) -> Vec<u8> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // TODO: handle invalid crypt
        let mut ciphertext = self
            .cipher()
            .encrypt(nonce, input)
            .expect("encryption failure");
        let mut nonce_and_ciphertext = nonce.to_vec();
        nonce_and_ciphertext.append(&mut ciphertext);
        nonce_and_ciphertext
    }

    pub fn decrypt(&self, nonce_and_ciphertext: &[u8]) -> Result<Vec<u8>> {
        let (nonce_bytes, ciphertext) = nonce_and_ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher()
            .decrypt(nonce, ciphertext)
            .map_err(|_| Error::InvalidInput("decryption error"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cipher_key = CipherKey::random();
        let cipher = ContentCipher::new(&cipher_key);
        let plaintext = b"Hello World";
        let ciphertext = cipher.encrypt(plaintext);
        assert_eq!(plaintext.to_vec(), cipher.decrypt(&ciphertext).unwrap());
    }

    #[test]
    fn bad_crypt() {
        let cipher_key = CipherKey::random();
        let cipher = ContentCipher::new(&cipher_key);
        let plaintext = b"Hello World";
        let mut ciphertext = cipher.encrypt(plaintext);
        ciphertext[0] = ciphertext[0] + 1;
        assert!(cipher.decrypt(&ciphertext).is_err());
    }
}
