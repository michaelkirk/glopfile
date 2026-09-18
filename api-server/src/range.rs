//! Parsing of the `Range` and `Content-Range` headers the transfer protocol uses.

use axum::http::header;
use axum::http::HeaderMap;

use crate::session::Position;

/// Where a downloader wants to resume from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadPosition {
    At(Position),
    /// The downloader has everything; `Range: bytes=-0`.
    Eof,
}

/// Parses `Range: bytes=<position>-`, the only form downloaders may send.
pub fn download_position(headers: &HeaderMap) -> Result<DownloadPosition, &'static str> {
    let Some(value) = headers.get(header::RANGE) else {
        return Ok(DownloadPosition::At(0));
    };
    let value = value.to_str().map_err(|_| "malformed range")?;
    let specs = value
        .strip_prefix("bytes=")
        .ok_or("unsupported range unit")?;
    let (start, end) = specs.split_once('-').ok_or("malformed range")?;
    if end.contains(',') {
        return Err("multiple ranges unimplemented");
    }

    match (start.trim(), end.trim()) {
        ("", "0") => Ok(DownloadPosition::Eof),
        ("", _) => Err("suffix ranges unimplemented"),
        (start, "") => Ok(DownloadPosition::At(
            start.parse().map_err(|_| "malformed range")?,
        )),
        _ => Err("partial ranges unimplemented"),
    }
}

/// Parses `Content-Range: bytes <position>-<last>/<size>`, defaulting to the start of
/// the content when the header is absent or the uploader has nothing left to send.
pub fn upload_position(headers: &HeaderMap) -> Result<Position, &'static str> {
    let Some(value) = headers.get(header::CONTENT_RANGE) else {
        return Ok(0);
    };
    let value = value.to_str().map_err(|_| "malformed content range")?;
    let spec = value
        .strip_prefix("bytes ")
        .ok_or("unsupported range unit")?
        .trim();
    let (range, size) = spec.rsplit_once('/').ok_or("malformed content range")?;

    if range == "*" {
        return Ok(0);
    }
    let (start, end) = range.split_once('-').ok_or("malformed content range")?;
    let start: Position = start.parse().map_err(|_| "malformed content range")?;
    let end: Position = end.parse().map_err(|_| "malformed content range")?;
    let size: Position = size.parse().map_err(|_| "malformed content range")?;

    if size != end + 1 {
        return Err("partial ranges unimplemented");
    }
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(name: header::HeaderName, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn download_ranges() {
        let at = |value| download_position(&headers(header::RANGE, value));
        assert_eq!(
            download_position(&HeaderMap::new()),
            Ok(DownloadPosition::At(0))
        );
        assert_eq!(at("bytes=0-"), Ok(DownloadPosition::At(0)));
        assert_eq!(at("bytes=42-"), Ok(DownloadPosition::At(42)));
        assert_eq!(at("bytes=-0"), Ok(DownloadPosition::Eof));
        assert!(at("bytes=0-10").is_err());
        assert!(at("bytes=-10").is_err());
        assert!(at("bytes=0-,20-").is_err());
        assert!(at("items=0-").is_err());
    }

    #[test]
    fn upload_ranges() {
        let at = |value| upload_position(&headers(header::CONTENT_RANGE, value));
        assert_eq!(upload_position(&HeaderMap::new()), Ok(0));
        assert_eq!(at("bytes */100"), Ok(0));
        assert_eq!(at("bytes 0-99/100"), Ok(0));
        assert_eq!(at("bytes 40-99/100"), Ok(40));
        assert!(at("bytes 0-49/100").is_err());
        assert!(at("bytes 0-99/*").is_err());
        assert!(at("items 0-99/100").is_err());
    }
}
