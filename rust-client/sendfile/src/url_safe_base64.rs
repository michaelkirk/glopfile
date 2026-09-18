use base64::engine::{general_purpose::STANDARD, Engine};

pub fn encode(bytes: &[u8]) -> String {
    let conventional_base64 = STANDARD.encode(bytes);
    conventional_base64
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            '=' => '~',
            c => c,
        })
        .collect::<String>()
}

pub fn decode(url_safe_base64: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let conventional_base64 = url_safe_base64
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            '~' => '=',
            c => c,
        })
        .collect::<String>();

    STANDARD.decode(conventional_base64)
}
