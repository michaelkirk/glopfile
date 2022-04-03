#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        type DefaultCipher = web::WebCipher;
    } else {
        type DefaultCipher = native::NativeCipher;
    }
}

use std::sync::Arc;

use aes_gcm::aead::Payload;
use aes_gcm::{Aes256Gcm, Key, Nonce};
use derive_more::Deref;
use zeroize::ZeroizeOnDrop;

use crate::url_safe_base64;
use crate::{Error, Result};

#[derive(Clone)]
pub struct CipherKey {
    bytes: Arc<CipherKeyBytes>,
}

#[derive(Deref, ZeroizeOnDrop)]
struct CipherKeyBytes([u8; 32]);

impl std::fmt::Debug for CipherKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("CipherKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[async_trait::async_trait(?Send)]
trait Cipher {
    async fn new(key: &Aes256GcmKey) -> Self
    where
        Self: Sized;
    async fn encrypt(&self, nonce: &Aes256GcmNonce, plaintext: Payload<'_, '_>) -> Vec<u8>;
    async fn decrypt(&self, nonce: &Aes256GcmNonce, ciphertext: Payload<'_, '_>) -> Result<Vec<u8>>;
}

type Aes256GcmKey = Key<<Aes256Gcm as aes_gcm::NewAead>::KeySize>;
type Aes256GcmNonce = Nonce<<Aes256Gcm as aes_gcm::AeadCore>::NonceSize>;

impl CipherKey {
    pub fn random() -> Self {
        Self { bytes: Arc::new(CipherKeyBytes(rand::random())) }
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes: Arc::new(CipherKeyBytes(bytes)) }
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    pub fn from_string(url_safe_base64: &str) -> Result<Self> {
        let bytes = url_safe_base64::decode(url_safe_base64)
            .map_err(|_| Error::InvalidCipherKey)?;

        let byte_array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::InvalidCipherKey)?;

        Ok(Self::from_bytes(byte_array))
    }

    pub fn serialized(&self) -> String {
        url_safe_base64::encode(self.bytes())
    }
}

pub(crate) struct ContentCipher {
    cipher_key: CipherKey,
}
impl ContentCipher {
    pub fn new(cipher_key: &CipherKey) -> Self {
        Self { cipher_key: cipher_key.clone() }
    }

    pub const fn extra_ciphertext_len() -> u64 {
        // TODO somehow use constants from aes_gcm here
        12 + 16
    }

    async fn cipher(&self) -> impl Cipher {
        let key = Key::from_slice(self.cipher_key.bytes());
        DefaultCipher::new(key).await
    }

    // TODO: stream
    pub async fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = self.cipher().await;
        let mut ciphertext = cipher.encrypt(nonce, plaintext.into()).await;
        let mut nonce_and_ciphertext = nonce.to_vec();
        nonce_and_ciphertext.append(&mut ciphertext);
        nonce_and_ciphertext
    }

    pub async fn decrypt(&self, nonce_and_ciphertext: &[u8]) -> Result<Vec<u8>> {
        let (nonce_bytes, ciphertext) = nonce_and_ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = self.cipher().await;
        cipher.decrypt(nonce, ciphertext.into()).await
    }
}
