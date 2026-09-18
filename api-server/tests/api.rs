//! End-to-end tests against a server bound to an ephemeral port.

use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use prost::Message as _;
use reqwest::{header, Client, StatusCode};
use sendfile_relay::api;
use sendfile_relay::protocol;
use sendfile_relay::registry::Registry;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

const NONEXISTENT_ID: &str = "1234";
const INVALID_ID: &str = "test_invalid_id";
const METADATA: &str = "test_metadata";
const CONTENT: &[u8] = b"test_content";

struct Server {
    base_url: String,
    client: Client,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = api::router(Registry::new());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            base_url: format!("http://{addr}"),
            client: Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn ws_url(&self, path: &str) -> String {
        format!("ws{}{path}", self.base_url.trim_start_matches("http"))
    }

    /// Provisions a session, returning its download id.
    async fn provision(&self) -> String {
        let response = self
            .client
            .post(self.url("/api/v1/files"))
            .form(&[("encrypted_metadata", METADATA)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.unwrap();
        body["download_id"].as_str().unwrap().to_owned()
    }

    async fn connect_websocket(
        &self,
        path: &str,
    ) -> impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
           + futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> {
        let (socket, _) = tokio_tungstenite::connect_async(self.ws_url(path))
            .await
            .unwrap();
        socket
    }
}

/// Gives concurrent requests time to reach the session before the next step.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn nonexistent_api_returns_404() {
    let server = Server::start().await;
    let response = server
        .client
        .get(server.url("/api/v1/test_nonexistent_api"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn provision_without_metadata_returns_400() {
    let server = Server::start().await;
    let response = server
        .client
        .post(server.url("/api/v1/files"))
        .form(&[("unrelated", "field")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn non_post_provision_returns_405() {
    let server = Server::start().await;
    let response = server
        .client
        .get(server.url("/api/v1/files"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn provision_returns_upload_url_and_download_id() {
    let server = Server::start().await;
    let response = server
        .client
        .post(server.url("/api/v1/files"))
        .form(&[("encrypted_metadata", METADATA)])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body: Value = response.json().await.unwrap();
    let download_id = body["download_id"].as_str().unwrap();
    assert_eq!(
        body["upload_url"].as_str().unwrap(),
        format!("/api/v1/upload/{download_id}")
    );
}

#[tokio::test]
async fn preflight_returns_cors_headers() {
    let server = Server::start().await;
    let response = server
        .client
        .request(reqwest::Method::OPTIONS, server.url("/api/v1/files"))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "*"
    );
    assert_eq!(
        headers.get(header::ACCESS_CONTROL_ALLOW_HEADERS).unwrap(),
        "Content-Range, Range"
    );
    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "no-cache, no-store"
    );
}

#[tokio::test]
async fn nonexistent_download_returns_404() {
    let server = Server::start().await;
    for path in [
        format!("/api/v1/download/{NONEXISTENT_ID}"),
        format!("/api/v1/download/{NONEXISTENT_ID}/content"),
        format!("/api/v1/content/{NONEXISTENT_ID}"),
    ] {
        let response = server.client.get(server.url(&path)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn invalid_download_id_returns_400() {
    let server = Server::start().await;
    for path in [
        format!("/api/v1/download/{INVALID_ID}"),
        format!("/api/v1/download/{INVALID_ID}/content"),
        format!("/api/v1/content/{INVALID_ID}"),
    ] {
        let response = server.client.get(server.url(&path)).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
}

#[tokio::test]
async fn non_get_download_returns_405() {
    let server = Server::start().await;
    let id = server.provision().await;
    let response = server
        .client
        .post(server.url(&format!("/api/v1/download/{id}")))
        .body("")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn download_returns_metadata() {
    let server = Server::start().await;
    let id = server.provision().await;

    let response = server
        .client
        .get(server.url(&format!("/api/v1/download/{id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response.json().await.unwrap();
    assert_eq!(body["meta"].as_str().unwrap(), METADATA);
    assert_eq!(
        body["encrypted_content_url"].as_str().unwrap(),
        format!("/api/v1/download/{id}/content")
    );
}

#[tokio::test]
async fn nonexistent_upload_returns_404() {
    let server = Server::start().await;
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{NONEXISTENT_ID}")))
        .body(CONTENT)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_upload_id_returns_400() {
    let server = Server::start().await;
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{INVALID_ID}")))
        .body(CONTENT)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn transfer_relays_content_to_the_downloader() {
    let server = Server::start().await;
    let id = server.provision().await;

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .body(CONTENT)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");

    let downloaded = download.await.unwrap();
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        downloaded.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
    assert_eq!(downloaded.bytes().await.unwrap(), CONTENT);
}

#[tokio::test]
async fn upload_at_the_wrong_position_returns_409() {
    let server = Server::start().await;
    let id = server.provision().await;

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let _download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });
    settle().await;

    let size = CONTENT.len();
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .header(
            header::CONTENT_RANGE,
            format!("bytes 5-{}/{size}", size - 1),
        )
        .body(&CONTENT[5..])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["position"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn resuming_a_download_delivers_the_remaining_content() {
    let server = Server::start().await;
    let id = server.provision().await;

    let offset = 5;
    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move {
        client
            .get(content_url)
            .header(header::RANGE, format!("bytes={offset}-"))
            .send()
            .await
            .unwrap()
    });
    settle().await;

    let size = CONTENT.len();
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .header(
            header::CONTENT_RANGE,
            format!("bytes {offset}-{}/{size}", size - 1),
        )
        .body(&CONTENT[offset..])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(
        download.await.unwrap().bytes().await.unwrap(),
        &CONTENT[offset..]
    );
}

#[tokio::test]
async fn eof_range_finishes_the_download() {
    let server = Server::start().await;
    let id = server.provision().await;

    let response = server
        .client
        .get(server.url(&format!("/api/v1/download/{id}/content")))
        .header(header::RANGE, "bytes=-0")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.bytes().await.unwrap().is_empty());

    // Finishing the download ends the session.
    settle().await;
    let response = server
        .client
        .get(server.url(&format!("/api/v1/download/{id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unsupported_ranges_return_400() {
    let server = Server::start().await;
    let id = server.provision().await;

    let response = server
        .client
        .get(server.url(&format!("/api/v1/download/{id}/content")))
        .header(header::RANGE, "bytes=0-5")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .header(header::CONTENT_RANGE, "bytes 0-5/100")
        .body(CONTENT)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn websocket_for_a_nonexistent_session_returns_404() {
    let server = Server::start().await;
    let response = server
        .client
        .get(server.url(&format!("/api/v1/download/{NONEXISTENT_ID}/ws")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn websockets_relay_frames_between_peers() {
    let server = Server::start().await;
    let id = server.provision().await;

    let mut uploader = server
        .connect_websocket(&format!("/api/v1/upload/{id}/ws"))
        .await;
    let mut downloader = server
        .connect_websocket(&format!("/api/v1/download/{id}/ws"))
        .await;

    uploader
        .send(Message::binary(b"to downloader".to_vec()))
        .await
        .unwrap();
    assert_eq!(
        next_data_frame(&mut downloader).await,
        Message::binary(b"to downloader".to_vec())
    );

    downloader.send(Message::text("to uploader")).await.unwrap();
    assert_eq!(
        next_data_frame(&mut uploader).await,
        Message::text("to uploader")
    );
}

#[tokio::test]
async fn uploader_websocket_receives_progress_acks() {
    let server = Server::start().await;
    let id = server.provision().await;

    let mut uploader = server
        .connect_websocket(&format!("/api/v1/upload/{id}/ws"))
        .await;

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });
    settle().await;

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .body(CONTENT)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    download.await.unwrap().bytes().await.unwrap();

    // The session acks the starting position, then each chunk it hands over.
    assert_eq!(next_ack(&mut uploader).await, 0);
    assert_eq!(next_ack(&mut uploader).await, CONTENT.len() as u64);
}

async fn next_data_frame<S>(socket: &mut S) -> Message
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("socket closed")
            .unwrap();
        if matches!(message, Message::Binary(_) | Message::Text(_)) {
            return message;
        }
    }
}

async fn next_ack<S>(socket: &mut S) -> u64
where
    S: futures::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let Message::Binary(data) = next_data_frame(socket).await else {
        panic!("expected a binary ack");
    };
    let message = protocol::WebSocketMessage::decode(data).unwrap();
    match message.inner.unwrap() {
        protocol::web_socket_message::Inner::UploadDataAck(ack) => ack.offset,
        other => panic!("expected an upload ack, got {other:?}"),
    }
}

#[tokio::test]
async fn transfer_relays_content_larger_than_one_chunk() {
    let server = Server::start().await;
    let id = server.provision().await;

    let content: Vec<u8> = (0..4 * 1024 * 1024u32).map(|i| i as u8).collect();

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .body(content.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(download.await.unwrap().bytes().await.unwrap(), content);
}

#[tokio::test]
async fn health_check_reports_ok() {
    let server = Server::start().await;
    let response = server
        .client
        .get(server.url("/api/v1/health_check"))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");
}

#[tokio::test]
async fn start_upload_accepts_the_expected_position() {
    let server = Server::start().await;
    let id = server.provision().await;

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}/start")))
        .form(&[("position", 0)])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");
}

#[tokio::test]
async fn start_upload_reports_the_downloader_position() {
    let server = Server::start().await;
    let id = server.provision().await;

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let _download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });
    settle().await;

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}/start")))
        .form(&[("position", 5)])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        response.json::<Value>().await.unwrap()["position"]
            .as_u64()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn start_upload_does_not_connect_an_uploader() {
    let server = Server::start().await;
    let id = server.provision().await;

    // Checking twice in a row must not look like a replaced connection.
    for _ in 0..2 {
        let response = server
            .client
            .post(server.url(&format!("/api/v1/upload/{id}/start")))
            .form(&[("position", 0)])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");
    }
}

#[tokio::test]
async fn start_upload_without_a_position_returns_400() {
    let server = Server::start().await;
    let id = server.provision().await;

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}/start")))
        .form(&[("unrelated", "field")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn start_upload_for_a_nonexistent_session_returns_404() {
    let server = Server::start().await;
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{NONEXISTENT_ID}/start")))
        .form(&[("position", 0)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_past_the_end_waits_for_the_downloader_to_finish() {
    let server = Server::start().await;
    let id = server.provision().await;

    // The downloader already holds the whole file and is waiting for the stream to end.
    let size = CONTENT.len();
    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move {
        client
            .get(content_url)
            .header(header::RANGE, format!("bytes={size}-"))
            .send()
            .await
            .unwrap()
    });
    settle().await;

    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .header(header::CONTENT_RANGE, format!("bytes */{size}"))
        .body(Vec::new())
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");
    assert!(download.await.unwrap().bytes().await.unwrap().is_empty());
}

/// Walks the exact request sequence rust-client's `upload_file` performs: ask
/// `/start` where to resume, POST the content there, then read the status body.
#[tokio::test]
async fn rust_client_upload_sequence() {
    let server = Server::start().await;
    let id = server.provision().await;

    let content_url = server.url(&format!("/api/v1/download/{id}/content"));
    let client = server.client.clone();
    let download = tokio::spawn(async move { client.get(content_url).send().await.unwrap() });
    settle().await;

    let size = CONTENT.len();
    let position = 0usize;

    let start = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}/start")))
        .form(&[("position", position)])
        .send()
        .await
        .unwrap();
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(start.json::<Value>().await.unwrap()["status"], "ok");

    let content_range = if position < size {
        format!("bytes {position}-{}/{size}", size - 1)
    } else {
        format!("bytes */{size}")
    };
    let response = server
        .client
        .post(server.url(&format!("/api/v1/upload/{id}")))
        .header(header::CONTENT_RANGE, content_range)
        .body(&CONTENT[position..])
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::CONFLICT);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.json::<Value>().await.unwrap()["status"], "ok");

    assert_eq!(download.await.unwrap().bytes().await.unwrap(), CONTENT);
}
