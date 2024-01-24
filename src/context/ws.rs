use std::sync::Arc;

use axum::{
    extract::{ws::Message, Path, State, WebSocketUpgrade},
    response::Response,
    routing::get,
};
use futures::StreamExt;

use crate::db::playlist::PlaylistId;

use super::app::{AppRouter, AppState};

pub fn ws_router() -> AppRouter {
    AppRouter::new().route("/watch/:id/ws", get(websocket_handler))
}

async fn websocket_handler(
    Path(playlist_id): Path<i32>,
    ws: WebSocketUpgrade,
    State(app): State<Arc<AppState>>,
) -> Response {
    let playlist_id = PlaylistId(playlist_id);
    ws.on_upgrade(move |socket| async move {
        let (sender, mut receiver) = socket.split();
        app.add_websocket(playlist_id, sender).await;
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(msg)) => app.handle_websocket_message(&msg, playlist_id).await,
                Err(err) => tracing::warn!("websocket error: {err}"),
                _ => {}
            }
        }
    })
}
