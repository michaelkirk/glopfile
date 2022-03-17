pub enum Transport {
    P2P,
    Relay,
    Both,
}

impl Transport {
    pub fn with_p2p_and_relay(is_p2p_enabled: bool, is_relay_enabled: bool) -> Option<Self> {
        match (is_p2p_enabled, is_relay_enabled) {
            (true, true) => Some(Self::Both),
            (true, false) => Some(Self::P2P),
            (false, true) => Some(Self::Relay),
            (false, false) => None,
        }
    }
}
