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

use std::io;
use std::num::NonZeroUsize;
use std::sync::Arc;

use aes_gcm::aead::generic_array::typenum::Unsigned;
use aes_gcm::Aes256Gcm;
use bytes::Bytes;
use derive_more::Deref;
use futures::{AsyncRead, AsyncReadExt};
use zeroize::ZeroizeOnDrop;

use crate::url_safe_base64;
use crate::{Error, Result};

#[derive(Clone)]
pub struct CipherKey {
    bytes: Arc<CipherKeyBytes>,
}

pub use buffer::ContentCipherBuffer;
pub(crate) use buffer::ContentCipherBufferPaddingType;

pub const KEY_SIZE: usize = <Aes256Gcm as aes_gcm::NewAead>::KeySize::USIZE;
pub const NONCE_SIZE: usize = <Aes256Gcm as aes_gcm::AeadCore>::NonceSize::USIZE;
pub const TAG_SIZE: usize = <Aes256Gcm as aes_gcm::AeadCore>::TagSize::USIZE;

#[derive(Deref, ZeroizeOnDrop)]
struct CipherKeyBytes([u8; KEY_SIZE]);

impl std::fmt::Debug for CipherKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("CipherKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[async_trait::async_trait(?Send)]
trait CipherImpl {
    async fn derive_new(base_key: &CipherKey, hkdf_info: &[u8]) -> Self
    where
        Self: Sized;
    async fn encrypt(&self, nonce_and_plaintext: ContentCipherBuffer, aad: Vec<u8>) -> Vec<u8>;
    async fn decrypt(
        &self,
        nonce_and_ciphertext: ContentCipherBuffer,
        aad: Vec<u8>,
    ) -> Result<Bytes>;
}

impl CipherKey {
    pub fn random() -> Self {
        Self { bytes: Arc::new(CipherKeyBytes(rand::random())) }
    }

    pub fn from_bytes(bytes: [u8; KEY_SIZE]) -> Self {
        Self { bytes: Arc::new(CipherKeyBytes(bytes)) }
    }

    pub fn bytes(&self) -> &[u8; KEY_SIZE] {
        &self.bytes
    }

    pub fn from_string(url_safe_base64: &str) -> Result<Self> {
        let bytes =
            url_safe_base64::decode(url_safe_base64).map_err(|_| Error::InvalidCipherKey)?;

        let byte_array: [u8; KEY_SIZE] = bytes.try_into().map_err(|_| Error::InvalidCipherKey)?;

        Ok(Self::from_bytes(byte_array))
    }

    pub fn serialized(&self) -> String {
        url_safe_base64::encode(self.bytes())
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum ContentCipherUsage {
    Content,
    Metadata,
    UploaderRtcSignaling,
    DownloaderRtcSignaling,
}

impl ContentCipherUsage {
    fn info(&self) -> &'static [u8] {
        match self {
            Self::Content => b"content",
            Self::Metadata => b"metadata",
            Self::UploaderRtcSignaling => b"uploader_rtc_signaling",
            Self::DownloaderRtcSignaling => b"downloader_rtc_signaling",
        }
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
        (NONCE_SIZE + TAG_SIZE) as u64
    }

    async fn derive_cipher(&self, info: &[u8]) -> impl CipherImpl {
        DefaultCipher::derive_new(&self.cipher_key, info).await
    }

    // TODO: stream
    pub async fn encrypt(
        &self,
        mut plaintext: ContentCipherBuffer,
        usage: ContentCipherUsage,
    ) -> Vec<u8> {
        *plaintext.parts_mut().nonce = rand::random();

        let cipher = self.derive_cipher(usage.info()).await;
        cipher.encrypt(plaintext, vec![]).await
    }

    pub async fn decrypt(
        &self,
        nonce_and_ciphertext: Vec<u8>,
        usage: ContentCipherUsage,
    ) -> Result<Bytes> {
        let nonce_and_ciphertext =
            ContentCipherBuffer::from_nonce_and_ciphertext(nonce_and_ciphertext)?;

        let cipher = self.derive_cipher(usage.info()).await;
        cipher.decrypt(nonce_and_ciphertext, vec![]).await
    }

    pub async fn decrypt_message<M: prost::Message + Default>(
        &self,
        nonce_and_ciphertext: Vec<u8>,
        usage: ContentCipherUsage,
    ) -> Result<M> {
        // Converting to Bytes allows prost to zero-copy decode.
        let message_data = Bytes::from(self.decrypt(nonce_and_ciphertext, usage).await?);
        M::decode_length_delimited(message_data)
            .map_err(|error| Error::InvalidPeerMessage { source: Box::new(error) })
    }
}

mod buffer {
    use super::*;

    pub struct ContentCipherBuffer {
        data: Vec<u8>,
    }

    pub(crate) struct ContentCipherBufferPartsMut<'a> {
        pub nonce: &'a mut [u8; NONCE_SIZE],
        pub data: ContentCipherBufferData<'a>,
    }

    pub(crate) struct ContentCipherBufferData<'a> {
        payload_and_tag: &'a mut [u8],
    }

    pub(crate) struct ContentCipherBufferCiphertextPartsMut<'a> {
        pub payload: &'a mut [u8],
        #[cfg_attr(target_arch = "wasm32", allow(unused))]
        pub tag: &'a mut [u8; TAG_SIZE],
    }

    pub(crate) enum ContentCipherBufferPaddingType {
        Zeroes { block_len: usize },
    }

    impl ContentCipherBuffer {
        pub async fn read_plaintext_to_end(
            mut plaintext: impl AsyncRead + Unpin,
            plaintext_len: usize,
        ) -> io::Result<Self> {
            let mut data = Vec::with_capacity(NONCE_SIZE + plaintext_len + TAG_SIZE);
            // Fill with random data to guard against accidental nonce-reuse.
            data.extend_from_slice(&rand::random::<[u8; NONCE_SIZE]>());
            // As of futures-util-0.3.21, AsyncReadExt::read_to_end performs terribly with large files, as it zeroes the
            // entire buffer before reading anything into it.
            let mut buf = [0; 16384];
            while let Some(read_len) = NonZeroUsize::new(plaintext.read(&mut buf).await?) {
                data.extend_from_slice(&buf[..read_len.get()]);
            }
            data.extend_from_slice(&[0; TAG_SIZE]);

            Ok(Self { data })
        }

        pub fn from_plaintext(plaintext: &[u8]) -> Self {
            let mut data = Vec::with_capacity(NONCE_SIZE + plaintext.len() + TAG_SIZE);
            // Fill with random data to guard against accidental nonce-reuse.
            data.extend_from_slice(&rand::random::<[u8; NONCE_SIZE]>());
            data.extend_from_slice(plaintext);
            data.extend_from_slice(&[0; TAG_SIZE]);

            Self { data }
        }

        pub(crate) fn from_message_padded<M: prost::Message>(
            message: &M,
            padding_type: ContentCipherBufferPaddingType,
        ) -> Self {
            let unpadded_len = {
                let encoded_message_len = message.encoded_len();
                prost::length_delimiter_len(encoded_message_len) + encoded_message_len
            };

            match padding_type {
                ContentCipherBufferPaddingType::Zeroes { block_len } => {
                    // This is an inlined usize::next_multiple_of as of writing, which is currently unstable.
                    let padded_len = match unpadded_len % block_len {
                        0 => unpadded_len,
                        r => unpadded_len + (block_len - r),
                    };

                    let mut data = Vec::with_capacity(NONCE_SIZE + padded_len + TAG_SIZE);
                    // Fill with random data to guard against accidental nonce-reuse.
                    data.extend_from_slice(&rand::random::<[u8; NONCE_SIZE]>());

                    message
                        .encode_length_delimited(&mut data)
                        .expect("Buffer has enough capacity");
                    // Fill in with zeroes up to the padded data length
                    data.resize(NONCE_SIZE + padded_len, 0);

                    data.extend_from_slice(&[0; TAG_SIZE]);

                    Self { data }
                }
            }
        }

        pub(super) fn from_nonce_and_ciphertext(data: Vec<u8>) -> Result<Self> {
            if data.len() < NONCE_SIZE + TAG_SIZE {
                Err(Error::Decrypt)
            } else {
                Ok(Self { data })
            }
        }

        pub(super) fn into_nonce_and_ciphertext(self) -> Vec<u8> {
            self.data
        }

        pub(super) fn into_plaintext(self) -> Bytes {
            let mut plaintext_tag_and_nonce = Bytes::from(self.data);
            let mut plaintext_and_tag = plaintext_tag_and_nonce.split_off(NONCE_SIZE);
            let plaintext = plaintext_and_tag.split_to(plaintext_and_tag.len() - TAG_SIZE);
            plaintext
        }

        pub(super) fn parts_mut(&mut self) -> ContentCipherBufferPartsMut<'_> {
            let (nonce, payload_and_tag) = self.data.split_at_mut(NONCE_SIZE);
            let nonce = nonce.try_into().unwrap_or_else(|_| unreachable!());
            let data = ContentCipherBufferData { payload_and_tag };
            ContentCipherBufferPartsMut { nonce, data }
        }
    }

    impl ContentCipherBufferData<'_> {
        pub(super) fn plaintext_mut(&mut self) -> &mut [u8] {
            self.ciphertext_parts_mut().payload
        }

        #[cfg_attr(not(target_arch = "wasm32"), allow(unused))]
        pub(super) fn ciphertext_mut(&mut self) -> &mut [u8] {
            self.payload_and_tag
        }

        pub(super) fn ciphertext_parts_mut(&mut self) -> ContentCipherBufferCiphertextPartsMut<'_> {
            let (payload, tag) = self
                .payload_and_tag
                .split_at_mut(self.payload_and_tag.len() - TAG_SIZE);
            let tag = tag.try_into().unwrap_or_else(|_| unreachable!());

            ContentCipherBufferCiphertextPartsMut { payload, tag }
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[tokio::test]
        async fn read_plaintext_to_end() {
            let plaintext = b"Hello World";
            let buffer = ContentCipherBuffer::read_plaintext_to_end(&plaintext[..], 0)
                .await
                .unwrap();
            assert_eq!(buffer.into_plaintext()[..], plaintext[..]);
        }

        #[test]
        fn from_into_plaintext() {
            let plaintext = b"Hello World";
            let mut buffer = ContentCipherBuffer::from_plaintext(plaintext);
            assert_eq!(buffer.parts_mut().data.plaintext_mut(), plaintext);
            assert_eq!(buffer.into_plaintext()[..], plaintext[..]);
        }

        #[test]
        fn from_into_ciphertext() {
            let payload = b"Hello World";
            let nonce = (32u8..).take(NONCE_SIZE).collect::<Vec<_>>();
            let tag = (0u8..).take(TAG_SIZE).collect::<Vec<_>>();
            let ciphertext = payload.iter().chain(&tag).cloned().collect::<Vec<_>>();
            let nonce_and_ciphertext = nonce.iter().chain(&ciphertext).cloned().collect::<Vec<_>>();
            let mut buffer =
                ContentCipherBuffer::from_nonce_and_ciphertext(nonce_and_ciphertext.clone())
                    .unwrap();
            assert_eq!(buffer.parts_mut().data.ciphertext_mut(), ciphertext);
            assert_eq!(
                buffer.parts_mut().data.ciphertext_parts_mut().payload,
                payload,
            );
            assert_eq!(buffer.parts_mut().data.ciphertext_parts_mut().tag, &tag[..]);
            assert_eq!(
                buffer.into_nonce_and_ciphertext()[..],
                nonce_and_ciphertext[..],
            );
        }

        #[test]
        fn from_plaintext_into_ciphertext() {
            let plaintext = b"Hello World";
            let mut buffer = ContentCipherBuffer::from_plaintext(plaintext);

            let nonce = (32u8..).take(NONCE_SIZE).collect::<Vec<_>>();
            let tag = (0u8..).take(TAG_SIZE).collect::<Vec<_>>();
            buffer.parts_mut().nonce.copy_from_slice(&nonce);
            let mut data = buffer.parts_mut().data;
            data.ciphertext_parts_mut().tag.copy_from_slice(&tag);

            let expected_nonce_and_ciphertext = (nonce.iter())
                .chain(plaintext)
                .chain(&tag)
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(
                buffer.into_nonce_and_ciphertext(),
                expected_nonce_and_ciphertext
            );
        }

        #[test]
        fn from_ciphertext_into_plaintext() {
            let ciphertext = b"Hello World";
            let nonce = (32u8..).take(NONCE_SIZE).collect::<Vec<_>>();
            let tag = (0u8..).take(TAG_SIZE).collect::<Vec<_>>();

            let nonce_and_ciphertext = (nonce.iter())
                .chain(ciphertext)
                .chain(&tag)
                .cloned()
                .collect::<Vec<_>>();

            let buffer =
                ContentCipherBuffer::from_nonce_and_ciphertext(nonce_and_ciphertext).unwrap();
            assert_eq!(buffer.into_plaintext()[..], ciphertext[..]);
        }
    }
}
