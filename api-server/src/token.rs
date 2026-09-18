use std::sync::atomic::{AtomicU64, Ordering};

/// Identifies one client connection, so a session can ignore a replaced peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token(u64);

impl Token {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for Token {
    fn default() -> Self {
        Self::new()
    }
}
