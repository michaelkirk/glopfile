pub fn encode(bytes: &[u8]) -> String {
    let conventional_base64 = base64::encode(bytes);
    conventional_base64
        .chars()
        .into_iter()
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
        .into_iter()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            '~' => '=',
            c => c,
        })
        .collect::<String>();

    base64::decode(conventional_base64)
}
