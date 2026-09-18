//! Relays websocket frames between the two peers of a session.

use std::pin::Pin;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt as _, StreamExt as _};
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant, Sleep};

use crate::session::{Direction, Frame, Peer, Session, WebsocketCommand, WebsocketPeer};
use crate::token::Token;

pub const MAX_FRAME_SIZE: usize = 102400;
const PING_INTERVAL: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn relay(socket: WebSocket, session: Session, direction: Direction) {
    let token = Token::new();
    let (commands, mut inbox) = mpsc::unbounded_channel();
    if session
        .start_websocket(direction, WebsocketPeer { token, commands })
        .await
        .is_none()
    {
        return;
    }
    let _disconnect = Disconnect {
        session: session.clone(),
        token,
        direction,
    };

    tracing::debug!(id = %session.id(), ?direction, "websocket connected");
    let (mut sink, mut frames) = socket.split();
    let mut ping = Box::pin(sleep(PING_INTERVAL));
    let mut idle = Box::pin(sleep(IDLE_TIMEOUT));

    loop {
        tokio::select! {
            command = inbox.recv() => match command {
                Some(WebsocketCommand::Send(frame)) => {
                    if sink.send(frame.into()).await.is_err() {
                        break;
                    }
                }
                Some(WebsocketCommand::SessionError) | None => break,
            },
            frame = frames.next() => {
                let Some(Ok(frame)) = frame else { break };
                reset(&mut ping, PING_INTERVAL);
                reset(&mut idle, IDLE_TIMEOUT);
                match frame {
                    Message::Text(text) => {
                        session.websocket_data(direction, Frame::Text(text.to_string()))
                    }
                    Message::Binary(data) => {
                        session.websocket_data(direction, Frame::Binary(data))
                    }
                    Message::Close(_) => break,
                    // Pings and pongs are answered by the transport.
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            () = &mut ping => {
                reset(&mut ping, PING_INTERVAL);
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
            () = &mut idle => {
                tracing::debug!(id = %session.id(), ?direction, "websocket idle timeout");
                break;
            }
        }
    }
    tracing::debug!(id = %session.id(), ?direction, "websocket disconnected");
}

fn reset(timer: &mut Pin<Box<Sleep>>, after: Duration) {
    timer.as_mut().reset(Instant::now() + after);
}

impl From<Frame> for Message {
    fn from(frame: Frame) -> Self {
        match frame {
            Frame::Text(text) => Message::Text(text.into()),
            Frame::Binary(data) => Message::Binary(data),
        }
    }
}

/// Unregisters the peer from its session when the relay ends.
struct Disconnect {
    session: Session,
    token: Token,
    direction: Direction,
}

impl Drop for Disconnect {
    fn drop(&mut self) {
        self.session
            .disconnected(self.token, Peer::Websocket(self.direction));
    }
}
