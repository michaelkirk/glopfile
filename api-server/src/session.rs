//! A transfer session, relaying content from one uploader to one downloader.
//!
//! Each session owns a task that serialises every operation on its state, so the
//! rendezvous between uploader and downloader needs no locking. Callers talk to it
//! through [`Session`].

use std::mem;
use std::ops::ControlFlow;

use bytes::Bytes;
use prost::Message as _;
use tokio::sync::{mpsc, oneshot};

use crate::id::SessionId;
use crate::protocol;
use crate::registry::Registry;
use crate::token::Token;

/// A byte offset into the content being transferred.
pub type Position = u64;

/// A chunk of uploaded content, and whether it completes the upload.
#[derive(Debug)]
pub enum Chunk {
    More(Bytes),
    Final(Bytes),
}

/// Content handed to a connected downloader.
#[derive(Debug)]
pub enum DownloadEvent {
    More(Bytes),
    Final(Bytes),
    ConnectionReplaced,
}

/// A websocket frame relayed between the two peers of a session.
#[derive(Clone, Debug)]
pub enum Frame {
    Text(String),
    Binary(Bytes),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadError {
    /// No such session, or the session has since ended.
    NotFound,
    /// Another uploader took over, or the download was torn down.
    ConnectionReplaced,
    /// The downloader is at a different offset; the uploader must resume from it.
    Position(Position),
    /// The download completed.
    Finished,
    /// The uploader sent another chunk before the previous one was accepted.
    DataPending,
}

/// What a session should send to a websocket peer.
#[derive(Debug)]
pub enum WebsocketCommand {
    Send(Frame),
    SessionError,
}

/// A websocket peer registered with a session.
#[derive(Debug)]
pub struct WebsocketPeer {
    pub token: Token,
    pub commands: mpsc::UnboundedSender<WebsocketCommand>,
}

/// Which peer of a session went away.
#[derive(Clone, Copy, Debug)]
pub enum Peer {
    /// `completed` is set when the downloader received the final chunk.
    Downloader {
        completed: bool,
    },
    Uploader,
    Websocket(Direction),
}

/// A handle to a running session task.
#[derive(Clone, Debug)]
pub struct Session {
    id: SessionId,
    commands: mpsc::UnboundedSender<Command>,
}

impl Session {
    pub fn spawn(id: SessionId, metadata: String, registry: Registry) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let state = State {
            id: id.clone(),
            metadata,
            downloader: DownloaderSlot::Disconnected,
            downloader_ws: None,
            uploader: None,
            uploader_ws: None,
            pending: Pending::None,
        };
        tokio::spawn(state.run(receiver, registry));
        Self { id, commands }
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub async fn metadata(&self) -> Option<String> {
        self.request(Command::Metadata).await
    }

    /// Connects a downloader at `position`, replacing any downloader already connected.
    pub async fn start_download(
        &self,
        token: Token,
        position: Position,
        events: mpsc::UnboundedSender<DownloadEvent>,
    ) -> Option<()> {
        self.request(|reply| Command::StartDownload {
            downloader: Downloader {
                token,
                position,
                events,
            },
            reply,
        })
        .await
    }

    /// Marks the download complete, ending the session once the uploader disconnects.
    pub async fn finish_download(&self) -> Option<()> {
        self.request(Command::FinishDownload).await
    }

    pub async fn start_upload(&self, token: Token, position: Position) -> Result<(), UploadError> {
        self.request(|reply| Command::StartUpload {
            token,
            position,
            reply,
        })
        .await
        .unwrap_or(Err(UploadError::NotFound))
    }

    pub async fn upload_data(&self, token: Token, chunk: Chunk) -> Result<(), UploadError> {
        self.request(|reply| Command::UploadData {
            token,
            chunk,
            reply,
        })
        .await
        .unwrap_or(Err(UploadError::NotFound))
    }

    /// Waits for any previously queued chunk to reach a downloader.
    pub async fn flush_upload(&self, token: Token) -> Result<(), UploadError> {
        self.request(|reply| Command::FlushUpload { token, reply })
            .await
            .unwrap_or(Err(UploadError::NotFound))
    }

    pub async fn start_websocket(&self, direction: Direction, peer: WebsocketPeer) -> Option<()> {
        self.request(|reply| Command::StartWebsocket {
            direction,
            peer,
            reply,
        })
        .await
    }

    pub fn websocket_data(&self, direction: Direction, frame: Frame) {
        let _ = self
            .commands
            .send(Command::WebsocketData { direction, frame });
    }

    pub fn disconnected(&self, token: Token, peer: Peer) {
        let _ = self.commands.send(Command::Disconnected { token, peer });
    }

    async fn request<T>(&self, command: impl FnOnce(oneshot::Sender<T>) -> Command) -> Option<T> {
        let (reply, response) = oneshot::channel();
        self.commands.send(command(reply)).ok()?;
        response.await.ok()
    }
}

enum Command {
    Metadata(oneshot::Sender<String>),
    StartDownload {
        downloader: Downloader,
        reply: oneshot::Sender<()>,
    },
    FinishDownload(oneshot::Sender<()>),
    StartUpload {
        token: Token,
        position: Position,
        reply: Waiter,
    },
    UploadData {
        token: Token,
        chunk: Chunk,
        reply: Waiter,
    },
    FlushUpload {
        token: Token,
        reply: Waiter,
    },
    StartWebsocket {
        direction: Direction,
        peer: WebsocketPeer,
        reply: oneshot::Sender<()>,
    },
    WebsocketData {
        direction: Direction,
        frame: Frame,
    },
    Disconnected {
        token: Token,
        peer: Peer,
    },
}

type Waiter = oneshot::Sender<Result<(), UploadError>>;

#[derive(Debug)]
struct Downloader {
    token: Token,
    position: Position,
    events: mpsc::UnboundedSender<DownloadEvent>,
}

#[derive(Debug)]
struct Uploader {
    token: Token,
    position: Position,
}

#[derive(Debug)]
enum DownloaderSlot {
    Disconnected,
    Connected(Downloader),
    Finished,
}

/// A chunk waiting for a downloader, or an error waiting for the uploader to ask.
enum Pending {
    None,
    Data {
        chunk: Chunk,
        /// Taken once the uploader waiting on this chunk has been answered.
        waiter: Option<Waiter>,
        position: Position,
        size: u64,
    },
    Error(UploadError),
}

struct State {
    id: SessionId,
    metadata: String,
    downloader: DownloaderSlot,
    downloader_ws: Option<WebsocketPeer>,
    uploader: Option<Uploader>,
    uploader_ws: Option<WebsocketPeer>,
    pending: Pending,
}

impl State {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>, registry: Registry) {
        tracing::debug!(id = %self.id, "session started");
        while let Some(command) = commands.recv().await {
            if self.handle(command).is_break() {
                break;
            }
        }
        tracing::debug!(id = %self.id, "session stopping");
        registry.remove(&self.id);
    }

    fn handle(&mut self, command: Command) -> ControlFlow<()> {
        match command {
            Command::Metadata(reply) => {
                let _ = reply.send(self.metadata.clone());
            }
            Command::StartDownload { downloader, reply } => {
                self.start_download(downloader);
                let _ = reply.send(());
            }
            Command::FinishDownload(reply) => return self.finish_download(reply),
            Command::StartUpload {
                token,
                position,
                reply,
            } => self.start_upload(token, position, reply),
            Command::UploadData {
                token,
                chunk,
                reply,
            } => self.upload_data(token, chunk, reply),
            Command::FlushUpload { token, reply } => self.flush_upload(token, reply),
            Command::StartWebsocket {
                direction,
                peer,
                reply,
            } => {
                self.start_websocket(direction, peer);
                let _ = reply.send(());
            }
            Command::WebsocketData { direction, frame } => self.relay_frame(direction, frame),
            Command::Disconnected { token, peer } => return self.disconnected(token, peer),
        }
        ControlFlow::Continue(())
    }

    fn start_download(&mut self, mut downloader: Downloader) {
        if let DownloaderSlot::Connected(replaced) =
            mem::replace(&mut self.downloader, DownloaderSlot::Disconnected)
        {
            let _ = replaced.events.send(DownloadEvent::ConnectionReplaced);
        }

        if let Pending::Data {
            chunk,
            waiter,
            position,
            size,
        } = mem::replace(&mut self.pending, Pending::None)
        {
            if downloader.position == position {
                downloader.position += size;
                let _ = downloader.events.send(chunk.into_event());
                send_upload_progress(&self.uploader_ws, downloader.position);
                reply_to(waiter, Ok(()));
            } else {
                reply_to(waiter, Err(UploadError::Position(downloader.position)));
            }
        }

        // An uploader sending from elsewhere in the file has to resume from here.
        if matches!(&self.uploader, Some(uploader) if uploader.position != downloader.position) {
            self.terminate_uploader(UploadError::Position(downloader.position));
        }

        self.downloader = DownloaderSlot::Connected(downloader);
    }

    fn finish_download(&mut self, reply: oneshot::Sender<()>) -> ControlFlow<()> {
        if let DownloaderSlot::Connected(replaced) =
            mem::replace(&mut self.downloader, DownloaderSlot::Finished)
        {
            let _ = replaced.events.send(DownloadEvent::ConnectionReplaced);
        }
        self.terminate_uploader(UploadError::Finished);
        let _ = reply.send(());

        if self.uploader.is_none() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn start_upload(&mut self, token: Token, position: Position, reply: Waiter) {
        if self.uploader.is_some() {
            self.terminate_uploader(UploadError::ConnectionReplaced);
            self.uploader = None;
        }

        if let DownloaderSlot::Connected(downloader) = &self.downloader {
            if position != downloader.position {
                let _ = reply.send(Err(UploadError::Position(downloader.position)));
                return;
            }
        }
        if let Pending::Data {
            position: pending,
            size,
            ..
        } = &self.pending
        {
            let expected = pending + size;
            if position != expected {
                let _ = reply.send(Err(UploadError::Position(expected)));
                return;
            }
        }

        if matches!(self.pending, Pending::Error(_)) {
            self.pending = Pending::None;
        }
        send_upload_progress(&self.uploader_ws, position);
        self.uploader = Some(Uploader { token, position });
        let _ = reply.send(Ok(()));
    }

    fn upload_data(&mut self, token: Token, chunk: Chunk, reply: Waiter) {
        if self.uploader_token() != Some(token) {
            let _ = reply.send(Err(UploadError::ConnectionReplaced));
            return;
        }
        match &self.pending {
            Pending::Data { .. } => {
                let _ = reply.send(Err(UploadError::DataPending));
                return;
            }
            Pending::Error(error) => {
                let _ = reply.send(Err(*error));
                return;
            }
            Pending::None => {}
        }

        let size = chunk.len();
        let uploader = self.uploader.as_mut().expect("uploader is connected");
        let position = uploader.position;
        uploader.position = position + size;

        match &mut self.downloader {
            DownloaderSlot::Connected(downloader) => {
                debug_assert_eq!(downloader.position, position);
                downloader.position = position + size;
                let _ = downloader.events.send(chunk.into_event());
                send_upload_progress(&self.uploader_ws, position + size);
                let _ = reply.send(Ok(()));
            }
            // Hold the chunk until a downloader shows up at this position.
            _ => {
                self.pending = Pending::Data {
                    chunk,
                    waiter: Some(reply),
                    position,
                    size,
                };
            }
        }
    }

    fn flush_upload(&mut self, token: Token, reply: Waiter) {
        if self.uploader_token() != Some(token) {
            let _ = reply.send(Err(UploadError::ConnectionReplaced));
            return;
        }
        match &mut self.pending {
            Pending::Error(error) => {
                let _ = reply.send(Err(*error));
            }
            Pending::Data { waiter, .. } => {
                *waiter = Some(reply);
            }
            Pending::None => {
                let _ = reply.send(Ok(()));
            }
        }
    }

    fn start_websocket(&mut self, direction: Direction, peer: WebsocketPeer) {
        if let Some(replaced) = self.websocket_mut(direction).replace(peer) {
            let _ = replaced.commands.send(WebsocketCommand::SessionError);
        }
    }

    fn relay_frame(&mut self, from: Direction, frame: Frame) {
        let to = match from {
            Direction::Upload => Direction::Download,
            Direction::Download => Direction::Upload,
        };
        if let Some(peer) = self.websocket_mut(to) {
            let _ = peer.commands.send(WebsocketCommand::Send(frame));
        }
    }

    fn disconnected(&mut self, token: Token, peer: Peer) -> ControlFlow<()> {
        match peer {
            Peer::Downloader { completed } => {
                if !matches!(&self.downloader, DownloaderSlot::Connected(d) if d.token == token) {
                    return ControlFlow::Continue(());
                }
                if completed {
                    self.downloader = DownloaderSlot::Finished;
                    return ControlFlow::Break(());
                }
                self.downloader = DownloaderSlot::Disconnected;
            }
            Peer::Uploader => {
                if self.uploader_token() != Some(token) {
                    return ControlFlow::Continue(());
                }
                self.uploader = None;
                if matches!(self.downloader, DownloaderSlot::Finished) {
                    return ControlFlow::Break(());
                }
                // The queued error was meant for the uploader that just left.
                if matches!(self.pending, Pending::Error(_)) {
                    self.pending = Pending::None;
                }
            }
            Peer::Websocket(direction) => {
                let slot = self.websocket_mut(direction);
                if slot.as_ref().map(|peer| peer.token) == Some(token) {
                    *slot = None;
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// Ends the current upload, either by answering the call it is blocked on or by
    /// queueing the error for its next call.
    fn terminate_uploader(&mut self, error: UploadError) {
        if self.uploader.is_none() {
            return;
        }
        if let Pending::Data { waiter, .. } = &mut self.pending {
            reply_to(waiter.take(), Err(error));
            self.uploader = None;
        } else {
            self.pending = Pending::Error(error);
        }
    }

    fn uploader_token(&self) -> Option<Token> {
        self.uploader.as_ref().map(|uploader| uploader.token)
    }

    fn websocket_mut(&mut self, direction: Direction) -> &mut Option<WebsocketPeer> {
        match direction {
            Direction::Upload => &mut self.uploader_ws,
            Direction::Download => &mut self.downloader_ws,
        }
    }
}

impl Chunk {
    fn len(&self) -> u64 {
        match self {
            Chunk::More(data) | Chunk::Final(data) => data.len() as u64,
        }
    }

    fn into_event(self) -> DownloadEvent {
        match self {
            Chunk::More(data) => DownloadEvent::More(data),
            Chunk::Final(data) => DownloadEvent::Final(data),
        }
    }
}

fn reply_to(waiter: Option<Waiter>, result: Result<(), UploadError>) {
    if let Some(waiter) = waiter {
        let _ = waiter.send(result);
    }
}

/// Tells the uploader's websocket how much content has reached the downloader.
fn send_upload_progress(uploader_ws: &Option<WebsocketPeer>, position: Position) {
    let Some(peer) = uploader_ws else { return };
    let message = protocol::WebSocketMessage {
        inner: Some(protocol::web_socket_message::Inner::UploadDataAck(
            protocol::UploadDataAck { offset: position },
        )),
    };
    let frame = Frame::Binary(message.encode_to_vec().into());
    let _ = peer.commands.send(WebsocketCommand::Send(frame));
}
