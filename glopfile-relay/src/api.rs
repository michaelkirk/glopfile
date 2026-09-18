//! The HTTP API. Paths and status codes here are the contract with the clients.

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::extract::rejection::FormRejection;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Form, Path, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::id::SessionId;
use crate::range::{self, DownloadPosition};
use crate::registry::Registry;
use crate::session::{Chunk, Direction, DownloadEvent, Peer, Session, UploadError};
use crate::token::Token;
use crate::websocket;

pub fn router(registry: Registry) -> Router {
    let api = Router::new()
        .route("/v1/health_check", get(health_check))
        .route("/v1/files", post(create_file))
        .route("/v1/download/{id}", get(download_meta))
        .route("/v1/download/{id}/content", get(download_content))
        .route("/v1/download/{id}/ws", any(download_websocket))
        .route("/v1/upload/{id}", post(upload))
        .route("/v1/upload/{id}/start", post(start_upload))
        .route("/v1/upload/{id}/ws", any(upload_websocket))
        .route("/v1/content/{id}", get(download_content))
        .fallback(|| async { ApiError::NotFound })
        .layer(axum::middleware::from_fn(common_headers))
        .with_state(registry);

    Router::new().nest("/api", api)
}

async fn health_check() -> Json<UploadResponse> {
    Json(UploadResponse::Ok)
}

#[derive(Deserialize)]
struct CreateFileRequest {
    encrypted_metadata: String,
}

#[derive(Serialize)]
struct CreateFileResponse {
    upload_url: String,
    download_id: String,
}

async fn create_file(
    State(registry): State<Registry>,
    form: Result<Form<CreateFileRequest>, FormRejection>,
) -> Result<Json<CreateFileResponse>, ApiError> {
    let Ok(Form(request)) = form else {
        return Err(ApiError::Invalid("metadata missing"));
    };
    let session = registry.create(request.encrypted_metadata);
    let id = session.id().encode();
    Ok(Json(CreateFileResponse {
        upload_url: format!("/api/v1/upload/{id}"),
        download_id: id,
    }))
}

#[derive(Serialize)]
struct DownloadMetaResponse {
    meta: String,
    encrypted_content_url: String,
}

async fn download_meta(
    State(registry): State<Registry>,
    Path(id): Path<String>,
) -> Result<Json<DownloadMetaResponse>, ApiError> {
    let session = lookup(&registry, &id)?;
    let meta = session.metadata().await.ok_or(ApiError::NotFound)?;
    Ok(Json(DownloadMetaResponse {
        encrypted_content_url: format!("/api/v1/download/{}/content", session.id()),
        meta,
    }))
}

async fn download_content(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = lookup(&registry, &id)?;
    let position = range::download_position(&headers).map_err(ApiError::Invalid)?;

    let position = match position {
        DownloadPosition::Eof => {
            session.finish_download().await.ok_or(ApiError::NotFound)?;
            return Ok(().into_response());
        }
        DownloadPosition::At(position) => position,
    };

    let token = Token::new();
    let (events, receiver) = mpsc::unbounded_channel();
    session
        .start_download(token, position, events)
        .await
        .ok_or(ApiError::NotFound)?;

    let download = Download {
        session,
        token,
        events: receiver,
        completed: false,
    };
    let content = futures::stream::unfold(download, |mut download| async move {
        if download.completed {
            return None;
        }
        match download.events.recv().await {
            Some(DownloadEvent::More(data)) => Some((Ok::<Bytes, Infallible>(data), download)),
            Some(DownloadEvent::Final(data)) => {
                download.completed = true;
                Some((Ok(data), download))
            }
            Some(DownloadEvent::ConnectionReplaced) | None => None,
        }
    });

    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Body::from_stream(content),
    )
        .into_response())
}

/// A connected downloader, which tells its session when the response ends.
struct Download {
    session: Session,
    token: Token,
    events: mpsc::UnboundedReceiver<DownloadEvent>,
    completed: bool,
}

impl Drop for Download {
    fn drop(&mut self) {
        self.session.disconnected(
            self.token,
            Peer::Downloader {
                completed: self.completed,
            },
        );
    }
}

#[derive(Serialize)]
struct UploadConflictResponse {
    position: u64,
}

/// The body of every upload response that is not a position conflict.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum UploadResponse {
    Ok,
    Error { reason: &'static str },
}

#[derive(Deserialize)]
struct StartUploadRequest {
    position: u64,
}

/// Lets an uploader check where to resume from before it starts sending content.
async fn start_upload(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    form: Result<Form<StartUploadRequest>, FormRejection>,
) -> Result<Response, ApiError> {
    let Ok(Form(request)) = form else {
        return Err(ApiError::Invalid("position missing"));
    };
    let session = lookup(&registry, &id)?;
    match session.check_start_upload(request.position).await {
        Ok(()) => Ok(Json(UploadResponse::Ok).into_response()),
        Err(error) => upload_error(error),
    }
}

async fn upload(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let session = lookup(&registry, &id)?;
    let position = range::upload_position(&headers).map_err(ApiError::Invalid)?;

    let token = Token::new();
    let _disconnect = Upload {
        session: session.clone(),
        token,
    };

    let started = match session.start_upload(token, position).await {
        Ok(()) => session.flush_upload(token).await,
        error => error,
    };
    if let Err(error) = started {
        return upload_error(error);
    }

    let mut chunks = body.into_data_stream().fuse();
    let mut next = chunks.next().await.transpose().map_err(read_failed)?;
    loop {
        let data = next.take().unwrap_or_default();
        next = chunks.next().await.transpose().map_err(read_failed)?;

        let last = next.is_none();
        let chunk = if last {
            Chunk::Final(data)
        } else {
            Chunk::More(data)
        };
        match session.upload_data(token, chunk).await {
            Ok(()) if last => return Ok(Json(UploadResponse::Ok).into_response()),
            Ok(()) => {}
            Err(error) => return upload_error(error),
        }
    }
}

/// Answers an upload request that the session would not accept. Only a position
/// mismatch is the uploader's problem; the rest mean the transfer is over.
fn upload_error(error: UploadError) -> Result<Response, ApiError> {
    let reason = match error {
        UploadError::NotFound => return Err(ApiError::NotFound),
        UploadError::Position(position) => {
            let conflict = Json(UploadConflictResponse { position });
            return Ok((StatusCode::CONFLICT, conflict).into_response());
        }
        // The content already reached the downloader, so the uploader is done.
        UploadError::Finished => return Ok(Json(UploadResponse::Ok).into_response()),
        UploadError::ConnectionReplaced => "connection_replaced",
        UploadError::DataPending => "data_pending",
    };
    Ok(Json(UploadResponse::Error { reason }).into_response())
}

fn read_failed(error: axum::Error) -> ApiError {
    tracing::debug!(%error, "upload body read failed");
    ApiError::Invalid("body read failed")
}

/// A connected uploader, which tells its session when the request ends.
struct Upload {
    session: Session,
    token: Token,
}

impl Drop for Upload {
    fn drop(&mut self) {
        self.session.disconnected(self.token, Peer::Uploader);
    }
}

async fn download_websocket(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    upgrade: Upgrade,
) -> Result<Response, ApiError> {
    websocket_upgrade(registry, &id, upgrade, Direction::Download)
}

async fn upload_websocket(
    State(registry): State<Registry>,
    Path(id): Path<String>,
    upgrade: Upgrade,
) -> Result<Response, ApiError> {
    websocket_upgrade(registry, &id, upgrade, Direction::Upload)
}

/// Deferred so that an unknown session is a 404 rather than a rejected upgrade.
type Upgrade = Result<WebSocketUpgrade, WebSocketUpgradeRejection>;

fn websocket_upgrade(
    registry: Registry,
    id: &str,
    upgrade: Upgrade,
    direction: Direction,
) -> Result<Response, ApiError> {
    let session = lookup(&registry, id)?;
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return Ok(rejection.into_response()),
    };
    Ok(upgrade
        .max_frame_size(websocket::MAX_FRAME_SIZE)
        .max_message_size(websocket::MAX_FRAME_SIZE)
        .on_upgrade(move |socket| websocket::relay(socket, session, direction)))
}

fn lookup(registry: &Registry, id: &str) -> Result<Session, ApiError> {
    let id = SessionId::decode(id).map_err(|_| ApiError::Invalid("invalid id"))?;
    registry.get(&id).ok_or(ApiError::NotFound)
}

enum ApiError {
    NotFound,
    Invalid(&'static str),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ApiError::Invalid(reason) => {
                tracing::info!(reason, "invalid request");
                StatusCode::BAD_REQUEST.into_response()
            }
        }
    }
}

/// Answers preflight requests and applies the headers every API response carries.
async fn common_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = if request.method() == Method::OPTIONS {
        StatusCode::OK.into_response()
    } else {
        next.run(request).await
    };

    let headers = response.headers_mut();
    for (name, value) in [
        (header::CACHE_CONTROL, "no-cache, no-store"),
        (header::EXPIRES, "Fri, 1 Jan 1999 12:00:00 AM GMT"),
        (header::PRAGMA, "no-cache"),
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        (
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, HEAD, POST, OPTIONS, PUT, PATCH, DELETE",
        ),
        (header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Range, Range"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers
        .entry(header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));
    response
}
