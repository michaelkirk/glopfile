use std::fmt;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use rand::Rng as _;

const ID_LENGTH: usize = 8;

/// Opaque identifier for a transfer session.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(Vec<u8>);

/// The URL-encoded form of a [`SessionId`].
#[derive(Debug)]
pub struct InvalidSessionId;

impl SessionId {
    pub fn generate() -> Self {
        let mut bytes = vec![0u8; ID_LENGTH];
        rand::rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Parses the URL-safe encoding produced by [`SessionId::encode`].
    pub fn decode(encoded: &str) -> Result<Self, InvalidSessionId> {
        let standard: String = encoded.chars().map(from_url_char).collect();
        let bytes = BASE64.decode(standard).map_err(|_| InvalidSessionId)?;
        if bytes.is_empty() {
            return Err(InvalidSessionId);
        }
        Ok(Self(bytes))
    }

    pub fn encode(&self) -> String {
        BASE64.encode(&self.0).chars().map(to_url_char).collect()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

fn to_url_char(ch: char) -> char {
    match ch {
        '+' => '-',
        '/' => '_',
        '=' => '~',
        ch => ch,
    }
}

fn from_url_char(ch: char) -> char {
    match ch {
        '-' => '+',
        '_' => '/',
        '~' => '=',
        ch => ch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let id = SessionId::generate();
        assert_eq!(SessionId::decode(&id.encode()).unwrap(), id);
    }

    #[test]
    fn encodes_url_safe_alphabet() {
        let id = SessionId(vec![0xfb, 0xff, 0xbf, 0x00]);
        assert_eq!(id.encode(), "-_-_AA~~");
        assert_eq!(SessionId::decode("-_-_AA~~").unwrap(), id);
    }

    #[test]
    fn rejects_malformed_ids() {
        assert!(SessionId::decode("test_invalid_id").is_err());
        assert!(SessionId::decode("").is_err());
    }
}
