use super::{
    playlist::playlist_router,
    ssr::ssr_router,
    static_files::static_file_router,
    ws::{ws_router, SocketId, SocketSink},
    ResponseResult,
};
use crate::{
    db::{
        establish_connection,
        media::{
            insert_media, insert_media_list, query_media_list_with_url, query_media_with_id,
            query_media_with_url, Media, MediaOrMediaList,
        },
        playlist::{query_playlist_from_id, update_playlist_current_item, PlaylistId},
        playlist_item::{query_playlist_item, PlaylistItem, PlaylistItemId},
        ResourceQueryError, ResourceQueryResult, SqliteConnectionPool,
    },
    resolvers::{normalize_media_url, resolve_media, resolve_media_list, MediaResolveError},
};
use anyhow::{Context, Result};
use axum::{extract::ws::Message, Router};
use diesel::{r2d2::ConnectionManager, SqliteConnection};
use futures::SinkExt;
use notify_rust::Notification;
use r2d2::PooledConnection;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::{
    collections::{hash_map::Entry, HashMap},
    process::Command,
    sync::Arc,
    thread::spawn,
    time::Duration,
};
use thiserror::Error;
use tokio::{runtime::Handle, sync::Mutex};
use tower::ServiceBuilder;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

#[derive(Clone, Copy)]
struct MediaControlState {
    playlist_id: PlaylistId,
    is_playing: bool,
}

impl MediaControlState {
    pub fn set_playing(&mut self, playing: bool) {
        self.is_playing = playing
    }

    pub fn toggle_playback(&mut self) -> bool {
        self.is_playing = !self.is_playing;
        self.is_playing
    }
}

pub struct AppState {
    db_pool: SqliteConnectionPool,
    sockets: Mutex<HashMap<PlaylistId, SocketSinkContainer>>,
    media_state: Mutex<Option<MediaControlState>>,
    media_controls: Option<Mutex<MediaControls>>,
}

pub type AppRouter = Router<Arc<AppState>>;

#[derive(Default)]
struct SocketSinkContainer {
    playing: HashMap<SocketId, SocketSink>,
    done: HashMap<SocketId, SocketSink>,
}

impl SocketSinkContainer {
    pub fn insert(&mut self, socket_id: SocketId, socket: SocketSink) {
        self.playing.insert(socket_id, socket);
    }

    pub fn remove(&mut self, socket_id: &SocketId) {
        self.playing.remove(socket_id);
        self.done.remove(socket_id);
    }

    pub fn len(&self) -> usize {
        self.playing.len() + self.done.len()
    }

    pub fn all_sockets(&mut self) -> impl Iterator<Item = (&SocketId, &mut SocketSink)> {
        self.playing.iter_mut().chain(self.done.iter_mut())
    }

    pub fn reset(&mut self) {
        self.playing.extend(std::mem::take(&mut self.done));
    }

    pub fn socket_done(&mut self, socket_id: SocketId) -> bool {
        if let Some(socket) = self.playing.remove(&socket_id) {
            self.done.insert(socket_id, socket);
        }

        if self.playing.is_empty() {
            self.reset();
            true
        } else {
            false
        }
    }
}

#[derive(Error, Debug)]
pub enum FetchMediaError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Resolve error: {0}")]
    ResolveError(MediaResolveError),
    #[error("Invalid url")]
    InvalidUrl(#[from] url::ParseError),
}

impl AppState {
    pub async fn new() -> Result<Arc<Self>> {
        let media_controls = MediaControls::new(PlatformConfig {
            dbus_name: "plst3",
            display_name: "plst3",
            // TODO: make this works on windows
            hwnd: None,
        })
        .map_err(|e| tracing::warn!("unable to create media controls: {e:?}"))
        .map(Mutex::new)
        .ok();

        let app = Arc::new(Self {
            db_pool: establish_connection()
                .context("unable to establish connection to database")?,
            sockets: Mutex::new(HashMap::new()),
            media_state: Mutex::new(Self::media_control_state_env()),
            media_controls,
        });

        if let Some(controls) = app.media_controls.as_ref() {
            let app = app.clone();
            let handle = Handle::current();
            controls
                .lock()
                .await
                .attach(move |event| {
                    handle.block_on(async {
                        app.handle_event(event)
                            .await
                            .map_err(|e| {
                                tracing::warn!("error handling media controls event: {e:?}")
                            })
                            .ok();
                    })
                })
                .map_err(|e| {
                    tracing::warn!("unable to attach event callback to media controls: {e:?}")
                })
                .ok();
        }

        app.update_media_metadata().await.ok();

        Ok(app)
    }

    async fn handle_event(self: &Arc<Self>, event: MediaControlEvent) -> Result<()> {
        let state = self.media_state.lock().await.as_ref().cloned();
        if let Some(MediaControlState { playlist_id, .. }) = state {
            match event {
                MediaControlEvent::Play => {
                    self.play(playlist_id).await;
                }
                MediaControlEvent::Pause => {
                    self.pause(playlist_id).await;
                }
                MediaControlEvent::Toggle => {
                    self.playpause(playlist_id).await;
                }
                MediaControlEvent::Next => {
                    let mut db_conn = self.acquire_db_connection()?;
                    self.next(&mut db_conn, playlist_id).await?;
                }
                MediaControlEvent::Previous => {
                    let mut db_conn = self.acquire_db_connection()?;
                    self.prev(&mut db_conn, playlist_id).await?;
                }
                MediaControlEvent::OpenUri(_) => todo!(),
                _ => {}
            }
        }

        Ok(())
    }

    pub fn create_router(self: Arc<Self>) -> Router {
        Router::new()
            .merge(playlist_router())
            .merge(ssr_router())
            .merge(static_file_router())
            .merge(ws_router())
            .with_state(self)
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new()),
            )
    }

    fn media_control_state_env() -> Option<MediaControlState> {
        std::env::var("CURRENT_PLAYLIST")
            .ok()?
            .parse::<PlaylistId>()
            .map_err(|e| tracing::warn!("unable to parse current playlist id: {e:?}"))
            .ok()
            .map(|playlist| MediaControlState {
                playlist_id: playlist,
                is_playing: true,
            })
    }

    pub fn acquire_db_connection(
        &self,
    ) -> Result<PooledConnection<ConnectionManager<SqliteConnection>>, r2d2::Error> {
        self.db_pool.get()
    }

    pub async fn fetch_media(
        &self,
        db_conn: &mut SqliteConnection,
        media_url: &str,
    ) -> Result<Media, FetchMediaError> {
        let media_url = normalize_media_url(media_url)
            .await
            .map_err(FetchMediaError::InvalidUrl)?;
        match query_media_with_url(db_conn, &media_url) {
            Ok(media) => return Ok(media),
            Err(ResourceQueryError::DatabaseError(e)) => {
                return Err(FetchMediaError::DatabaseError(e))
            }
            _ => {}
        }

        let media = resolve_media(&media_url)
            .await
            .map_err(FetchMediaError::ResolveError)?;
        insert_media(db_conn, media).map_err(FetchMediaError::DatabaseError)
    }

    pub async fn fetch_medias(
        &self,
        db_conn: &mut SqliteConnection,
        media_url: &str,
    ) -> Result<MediaOrMediaList, FetchMediaError> {
        let media_url = normalize_media_url(media_url)
            .await
            .map_err(FetchMediaError::InvalidUrl)?;
        tracing::info!("fetching media with url: {media_url}");
        match query_media_with_url(db_conn, &media_url) {
            Ok(media) => return Ok(media.into()),
            Err(ResourceQueryError::DatabaseError(e)) => {
                return Err(FetchMediaError::DatabaseError(e))
            }
            _ => {}
        }
        match query_media_list_with_url(db_conn, &media_url) {
            Ok(media_list) => return Ok(media_list.into()),
            Err(ResourceQueryError::DatabaseError(e)) => {
                return Err(FetchMediaError::DatabaseError(e))
            }
            _ => {}
        }

        let mut unsupported = false;
        let mut invalid = false;
        let mut not_found = false;
        match resolve_media(&media_url).await {
            Ok(media) => {
                return insert_media(db_conn, media)
                    .map(Into::into)
                    .map_err(FetchMediaError::DatabaseError)
            }
            Err(e) if matches!(e, MediaResolveError::FailedProcessing(_)) => {
                return Err(FetchMediaError::ResolveError(e))
            }
            Err(MediaResolveError::UnsupportedUrl) => unsupported = true,
            Err(MediaResolveError::InvalidMedia) => invalid = true,
            Err(MediaResolveError::MediaNotFound) => not_found = true,
            _ => {}
        };

        match resolve_media_list(&media_url).await {
            Ok((mut media_list, media_urls)) => {
                let mut media_ids = Vec::with_capacity(media_urls.len());
                for media_url in media_urls {
                    let id = self.fetch_media(db_conn, &media_url).await?.id;
                    media_ids.push(id);
                }
                media_list.media_ids = media_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
                    .into();
                return insert_media_list(db_conn, media_list)
                    .map(Into::into)
                    .map_err(FetchMediaError::DatabaseError);
            }
            Err(e) if matches!(e, MediaResolveError::FailedProcessing(_)) => {
                return Err(FetchMediaError::ResolveError(e))
            }
            Err(MediaResolveError::UnsupportedUrl) => unsupported = true,
            Err(MediaResolveError::InvalidMedia) => invalid = true,
            Err(MediaResolveError::MediaNotFound) => not_found = true,
            _ => {}
        };

        if not_found {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::MediaNotFound,
            ))
        } else if invalid {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::InvalidMedia,
            ))
        } else if unsupported {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::UnsupportedUrl,
            ))
        } else {
            unreachable!("either one of the above must be true")
        }
    }

    pub async fn set_current_playlist(&self, id: Option<PlaylistId>) {
        *self.media_state.lock().await = id.map(|id| MediaControlState {
            playlist_id: id,
            is_playing: true,
        });
    }

    pub async fn add_websocket(
        &self,
        playlist_id: PlaylistId,
        socket_id: SocketId,
        socket: SocketSink,
    ) {
        tracing::info!("WebSocket with id {socket_id} added");
        match self.sockets.lock().await.entry(playlist_id) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(Default::default()),
        }
        .insert(socket_id, socket);
    }

    pub async fn remove_websocket(&self, playlist_id: PlaylistId, socket_id: SocketId) {
        tracing::info!("WebSocket with id {socket_id} removed");
        if let Some(s) = self.sockets.lock().await.get_mut(&playlist_id) {
            s.remove(&socket_id)
        }
    }

    pub async fn send_message(&self, playlist_id: PlaylistId, message: &str) {
        tracing::info!("Message sent: {message}");
        if let Some(sockets) = self.sockets.lock().await.get_mut(&playlist_id) {
            let mut dead_ids = Vec::new();
            for (id, socket) in sockets.all_sockets() {
                if let Err(err) = socket.send(Message::Text(message.to_owned())).await {
                    tracing::info!("closing WebSocket id {id} due to error: {err}");
                    dead_ids.push(*id);
                }
            }

            for id in dead_ids.iter() {
                sockets.remove(id);
            }
        }
    }

    pub async fn refresh_playlist(&self, playlist_id: PlaylistId) {
        self.send_message(playlist_id, "refresh-playlist").await;
    }

    fn trigger_wm_update() {
        spawn(|| {
            Command::new("killall")
                .arg("-USR1")
                .arg("i3status")
                .spawn()
                .ok();
        });
    }

    async fn update_media_metadata(&self) -> Result<()> {
        if let Some(controls) = self.media_controls.as_ref() {
            let mut controls = controls.lock().await;
            if let Some(state) = self.media_state.lock().await.as_ref() {
                controls
                    .set_playback(if state.is_playing {
                        MediaPlayback::Playing { progress: None }
                    } else {
                        MediaPlayback::Paused { progress: None }
                    })
                    .ok();
                let mut db_conn = self.acquire_db_connection()?;
                let media = Self::get_current_media(&mut db_conn, state.playlist_id).await?;
                let media = media.as_ref();
                controls
                    .set_metadata(MediaMetadata {
                        title: media.map(|m| m.title.as_str()),
                        artist: media.map(|m| m.artist.as_str()),
                        album: None,
                        cover_url: None,
                        duration: media.and_then(|m| m.duration).map(|d| {
                            Duration::new(
                                d.whole_seconds().max(0) as u64,
                                d.subsec_nanoseconds().max(0) as u32,
                            )
                        }),
                    })
                    .ok();
                Self::trigger_wm_update();
            }
        }

        Ok(())
    }

    pub async fn media_changed(
        self: &Arc<Self>,
        playlist_id: PlaylistId,
        media: Option<&Media>,
    ) -> Result<()> {
        if let Some(sockets) = self.sockets.lock().await.get_mut(&playlist_id) {
            sockets.reset();
        }
        self.send_message(playlist_id, "media-changed").await;
        if Some(playlist_id)
            == self
                .media_state
                .lock()
                .await
                .as_ref()
                .map(|s| s.playlist_id)
        {
            self.update_media_metadata()
                .await
                .map_err(|e| {
                    tracing::warn!("unable to update media metadata: {e}");
                })
                .ok();
        }
        if let Some(media) = media {
            self.notify_playlist_item_change(playlist_id, &media);
        }
        Ok(())
    }
    pub async fn play(&self, playlist_id: PlaylistId) {
        if let Some(s) = self
            .media_state
            .lock()
            .await
            .as_mut()
            .filter(|s| s.playlist_id == playlist_id)
        {
            s.set_playing(true)
        }
        self.update_media_metadata().await.ok();
        self.send_message(playlist_id, "play").await
    }
    pub async fn pause(&self, playlist_id: PlaylistId) {
        if let Some(s) = self
            .media_state
            .lock()
            .await
            .as_mut()
            .filter(|s| s.playlist_id == playlist_id)
        {
            s.set_playing(false)
        }
        self.update_media_metadata().await.ok();
        self.send_message(playlist_id, "pause").await
    }
    pub async fn playpause(&self, playlist_id: PlaylistId) {
        let message = if let Some(s) = self
            .media_state
            .lock()
            .await
            .as_mut()
            .filter(|s| s.playlist_id == playlist_id)
        {
            if s.toggle_playback() {
                "play"
            } else {
                "pause"
            }
        } else {
            "playpause"
        };
        self.update_media_metadata().await.ok();
        self.send_message(playlist_id, message).await
    }

    pub fn get_current_item(
        db_conn: &mut SqliteConnection,
        playlist_id: PlaylistId,
    ) -> ResourceQueryResult<Option<PlaylistItem>> {
        let item_id = query_playlist_from_id(db_conn, playlist_id)?.current_item;
        if let Some(item_id) = item_id {
            Ok(Some(query_playlist_item(db_conn, item_id)?))
        } else {
            Ok(None)
        }
    }

    pub async fn get_current_media(
        db_conn: &mut SqliteConnection,
        playlist_id: PlaylistId,
    ) -> ResourceQueryResult<Option<Media>> {
        let item = Self::get_current_item(db_conn, playlist_id)?;
        if let Some(item) = item {
            Ok(Some(query_media_with_id(db_conn, item.media_id)?))
        } else {
            Ok(None)
        }
    }

    pub async fn set_playlist_item_as_current(
        self: &Arc<Self>,
        db_conn: &mut SqliteConnection,
        playlist_id: Option<PlaylistId>,
        item_id: PlaylistItemId,
    ) -> ResponseResult<()> {
        let playlist_id = match playlist_id {
            Some(id) => id,
            None => query_playlist_item(db_conn, item_id)?.playlist_id,
        };
        update_playlist_current_item(db_conn, playlist_id, Some(item_id))?;
        let item = query_playlist_item(db_conn, item_id)?;
        let media = query_media_with_id(db_conn, item.media_id)?;
        self.media_changed(playlist_id, Some(&media)).await?;
        Ok(())
    }

    pub async fn next(
        self: &Arc<Self>,
        db_conn: &mut SqliteConnection,
        playlist_id: PlaylistId,
    ) -> ResponseResult<()> {
        if let Some(current_item) = Self::get_current_item(db_conn, playlist_id)? {
            if let Some(next) = current_item.next {
                self.set_playlist_item_as_current(db_conn, Some(playlist_id), next)
                    .await?;
            } else if let Some(item) =
                query_playlist_from_id(db_conn, playlist_id)?.first_playlist_item
            {
                self.set_playlist_item_as_current(db_conn, Some(playlist_id), item)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn prev(
        self: &Arc<Self>,
        db_conn: &mut SqliteConnection,
        playlist_id: PlaylistId,
    ) -> ResponseResult<()> {
        if let Some(current_item) = Self::get_current_item(db_conn, playlist_id)? {
            if let Some(prev) = current_item.prev {
                self.set_playlist_item_as_current(db_conn, Some(playlist_id), prev)
                    .await?;
            } else if let Some(item) =
                query_playlist_from_id(db_conn, playlist_id)?.last_playlist_item
            {
                self.set_playlist_item_as_current(db_conn, Some(playlist_id), item)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn handle_websocket_message(
        self: &Arc<Self>,
        message: &str,
        playlist_id: PlaylistId,
        socket_id: SocketId,
    ) -> Result<()> {
        let mut db_conn = self.acquire_db_connection()?;
        match message {
            "next" => {
                if self
                    .sockets
                    .lock()
                    .await
                    .get_mut(&playlist_id)
                    .map(|sockets| sockets.socket_done(socket_id))
                    .unwrap_or_default()
                {
                    self.next(&mut db_conn, playlist_id).await?;
                }
            }
            "play" => self.play(playlist_id).await,
            "pause" => self.pause(playlist_id).await,
            m => tracing::warn!("unrecognizable message: {m}"),
        }
        Ok(())
    }

    pub async fn get_num_clients(&self, playlist_id: PlaylistId) -> usize {
        self.sockets
            .lock()
            .await
            .get(&playlist_id)
            .map(SocketSinkContainer::len)
            .unwrap_or_default()
    }

    pub fn notify_playlist_add(
        self: &Arc<Self>,
        playlist_id: PlaylistId,
        medias: &MediaOrMediaList,
        item_id: PlaylistItemId,
    ) {
        let body = match medias {
            MediaOrMediaList::Media(media) => format!("{} - {}", media.artist, media.title),
            MediaOrMediaList::MediaList(media_list) => {
                format!(
                    "{} - {}",
                    media_list.title.as_deref().unwrap_or("No title"),
                    media_list.artist.as_deref().unwrap_or("No artist")
                )
            }
        };
        let arc_self = self.clone();
        tokio::task::spawn_blocking(move || {
            match Notification::new()
                .summary(&format!("Media added to playlist {playlist_id}"))
                .body(&body)
                .action("default", "Go to media")
                .icon("/home/torani/dev/plst3/dist/assets/plst_notify.png")
                .show()
            {
                Ok(n) => {
                    n.wait_for_action(move |action| {
                        if action == "default" {
                            tokio::spawn(async move {
                                if let Ok(mut db_conn) =
                                    arc_self.acquire_db_connection().map_err(|e| {
                                        tracing::warn!("unable to acquire db connection: {e}")
                                    })
                                {
                                    tracing::info!("changing current media to item {item_id}");
                                    arc_self
                                        .set_playlist_item_as_current(
                                            &mut db_conn,
                                            Some(playlist_id),
                                            item_id,
                                        )
                                        .await
                                        .map_err(|e| {
                                            tracing::warn!("unable to change current media: {e}")
                                        })
                                        .ok();
                                }
                            });
                        }
                    });
                }
                Err(err) => {
                    tracing::warn!("unable to send notification for playlist media added: {err}")
                }
            }
        });
    }

    pub fn notify_playlist_item_change(self: &Arc<Self>, playlist_id: PlaylistId, media: &Media) {
        let body = format!("{} - {}", media.artist, media.title);
        tokio::task::spawn_blocking(move || {
            Notification::new()
                .summary(&format!("Media changed in playlist {playlist_id}"))
                .body(&body)
                .icon("/home/torani/dev/plst3/dist/assets/plst_notify.png")
                .show()
                .ok()
        });
    }
}
