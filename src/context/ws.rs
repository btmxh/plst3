use std::{
    fmt::Display,
    sync::{atomic::AtomicI32, Arc},
};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::Response,
    routing::get,
};
use futures::{stream::SplitSink, StreamExt};

use crate::db::playlist::PlaylistId;

use super::app::{AppRouter, AppState};

pub fn ws_router() -> AppRouter {
    AppRouter::new().route("/watch/:id/ws", get(websocket_handler))
}

#[derive(PartialEq, Eq, Clone, Copy, Hash)]
pub struct SocketId(pub i32);

impl Display for SocketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl SocketId {
    pub fn new() -> Self {
        static ID: AtomicI32 = AtomicI32::new(0);
        Self(ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl Default for SocketId {
    fn default() -> Self {
        Self::new()
    }
}

pub type SocketSink = SplitSink<WebSocket, Message>;

async fn websocket_handler(
    Path(playlist_id): Path<i32>,
    ws: WebSocketUpgrade,
    State(app): State<Arc<AppState>>,
) -> Response {
    let playlist_id = PlaylistId(playlist_id);
    ws.on_upgrade(move |socket| async move {
        let socket_id = SocketId::new();
        let (sender, mut receiver) = socket.split();
        app.add_websocket(playlist_id, socket_id, sender).await;
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(msg)) => {
                    app.handle_websocket_message(&msg, playlist_id, socket_id)
                        .await
                        .map_err(|e| tracing::warn!("error handling websocket message: {e}"))
                        .ok();
                }
                Err(err) => tracing::warn!("websocket error: {err}"),
                _ => {}
            }
        }
        tracing::info!("removing websocket of id {socket_id}");
        app.remove_websocket(playlist_id, socket_id).await;
    })
}
