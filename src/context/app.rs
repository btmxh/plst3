use super::{playlist::playlist_router, ssr::ssr_router, static_files::static_file_router};
use crate::{
    db::{
        establish_connection,
        media::{
            insert_media, insert_media_list, normalize_media_url, query_media_list_with_url,
            query_media_with_url, MediaId, MediaIdList, MediaIds,
        },
        playlist::{query_playlist_from_id, Playlist, PlaylistId},
        SqliteConnectionPool,
    },
    resolvers::{resolve_media, resolve_media_list, MediaResolveError},
};
use anyhow::{Context, Result};
use axum::Router;
use diesel::{r2d2::ConnectionManager, SqliteConnection};
use r2d2::PooledConnection;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

pub struct AppState {
    db_pool: SqliteConnectionPool,
    pub current_playlist: Mutex<Option<PlaylistId>>,
}

#[cfg(mpirs)]
#[async_trait]
impl RootInterface for AppState {
    async fn raise(&self) -> fdo::Result<()> {
        Ok(())
    }

    async fn quit(&self) -> fdo::Result<()> {
        Ok(())
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        println!("CanQuit");
        Ok(true)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _: bool) -> Result<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok("plst3".to_string())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok("io.github.btmxh.plst3".to_string())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![
            "file".to_string(),
            "http".to_string(),
            "https".to_string(),
        ])
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(vec![])
    }
}

#[cfg(mpirs)]
#[async_trait]
impl PlayerInterface for AppState {
    async fn next(&self) -> fdo::Result<()> {
        println!("Next");
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        println!("Previous");
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        println!("Pause");
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        println!("PlayPause");
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        println!("Stop");
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        println!("Play");
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        println!("Seek({:?})", offset);
        Ok(())
    }

    async fn set_position(&self, track_id: TrackId, position: Time) -> fdo::Result<()> {
        println!("SetPosition({}, {:?})", track_id, position);
        Ok(())
    }

    async fn open_uri(&self, uri: String) -> fdo::Result<()> {
        println!("OpenUri({})", uri);
        Ok(())
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        println!("PlaybackStatus");
        Ok(PlaybackStatus::Playing)
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        println!("LoopStatus");
        Ok(LoopStatus::None)
    }

    async fn set_loop_status(&self, loop_status: LoopStatus) -> Result<()> {
        println!("SetLoopStatus({})", loop_status);
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        println!("Rate");
        Ok(PlaybackRate::default())
    }

    async fn set_rate(&self, rate: PlaybackRate) -> Result<()> {
        println!("SetRate({})", rate);
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        println!("Shuffle");
        Ok(false)
    }

    async fn set_shuffle(&self, shuffle: bool) -> Result<()> {
        println!("SetShuffle({})", shuffle);
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        println!("Metadata");
        Ok(Metadata::default())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        println!("Volume");
        Ok(Volume::default())
    }

    async fn set_volume(&self, volume: Volume) -> Result<()> {
        println!("SetVolume({})", volume);
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        println!("Position");
        Ok(Time::ZERO)
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        println!("MinimumRate");
        Ok(PlaybackRate::default())
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        println!("MaximumRate");
        Ok(PlaybackRate::default())
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        println!("CanGoNext");
        Ok(false)
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        println!("CanGoPrevious");
        Ok(false)
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        println!("CanPlay");
        Ok(true)
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        println!("CanPause");
        Ok(true)
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        println!("CanSeek");
        Ok(false)
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        println!("CanControl");
        Ok(true)
    }
}

#[async_trait]
#[cfg(mpirs)]
impl TrackListInterface for AppState {
    async fn get_tracks_metadata(&self, track_ids: Vec<TrackId>) -> fdo::Result<Vec<Metadata>> {
        println!("GetTracksMetadata({:?})", track_ids);
        Ok(vec![])
    }

    async fn add_track(
        &self,
        uri: Uri,
        after_track: TrackId,
        set_as_current: bool,
    ) -> fdo::Result<()> {
        println!("AddTrack({}, {}, {})", uri, after_track, set_as_current);
        Ok(())
    }

    async fn remove_track(&self, track_id: TrackId) -> fdo::Result<()> {
        println!("RemoveTrack({})", track_id);
        Ok(())
    }

    async fn go_to(&self, track_id: TrackId) -> fdo::Result<()> {
        println!("GoTo({})", track_id);
        Ok(())
    }

    async fn tracks(&self) -> fdo::Result<Vec<TrackId>> {
        println!("Tracks");
        Ok(vec![])
    }

    async fn can_edit_tracks(&self) -> fdo::Result<bool> {
        println!("CanEditTracks");
        Ok(true)
    }
}

pub type AppRouter = Router<Arc<AppState>>;

#[derive(Error, Debug)]
pub enum FetchMediaError {
    #[error("Database error: {0}")]
    DatabaseError(anyhow::Error),
    #[error("Resolve error: {0}")]
    ResolveError(MediaResolveError),
    #[error("Invalid url")]
    InvalidUrl(#[from] url::ParseError),
}

impl AppState {
    pub async fn new() -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            db_pool: establish_connection()
                .context("unable to establish connection to database")?,
            current_playlist: Mutex::new(Self::current_playlist_env()),
        }))
    }

    pub fn create_router(self: Arc<Self>) -> Router {
        Router::new()
            .merge(playlist_router())
            .merge(ssr_router())
            .merge(static_file_router())
            .with_state(self)
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CompressionLayer::new()),
            )
    }

    fn current_playlist_env() -> Option<PlaylistId> {
        std::env::var("CURRENT_PLAYLIST")
            .ok()?
            .parse::<PlaylistId>()
            .map_err(|e| tracing::warn!("unable to parse current playlist id: {e:?}"))
            .ok()
    }

    pub fn acquire_db_connection(
        &self,
    ) -> anyhow::Result<PooledConnection<ConnectionManager<SqliteConnection>>> {
        self.db_pool
            .get()
            .context("unable to acquire DB connection")
    }

    pub async fn fetch_media(
        &self,
        db_conn: &mut SqliteConnection,
        media_url: &str,
    ) -> Result<MediaId, FetchMediaError> {
        let media_url = normalize_media_url(media_url).map_err(FetchMediaError::InvalidUrl)?;
        if let Some(media) =
            query_media_with_url(db_conn, &media_url).map_err(FetchMediaError::DatabaseError)?
        {
            return Ok(media.id);
        }

        let media = resolve_media(&media_url)
            .await
            .map_err(FetchMediaError::ResolveError)?;
        let id = insert_media(db_conn, media).map_err(FetchMediaError::DatabaseError)?;
        Ok(id)
    }

    pub async fn fetch_medias(
        &self,
        db_conn: &mut SqliteConnection,
        media_url: &str,
    ) -> Result<MediaIds, FetchMediaError> {
        let media_url = normalize_media_url(media_url).map_err(FetchMediaError::InvalidUrl)?;
        if let Some(media) =
            query_media_with_url(db_conn, &media_url).map_err(FetchMediaError::DatabaseError)?
        {
            return Ok(MediaIds::new_single(media.id));
        }
        if let Some(media_list) = query_media_list_with_url(db_conn, &media_url)
            .map_err(FetchMediaError::DatabaseError)?
        {
            return Ok(MediaIds::new_multiple(media_list.id, media_list.media_ids));
        }

        let mut unsupported = false;
        let mut invalid = false;
        let mut not_found = false;
        match resolve_media(&media_url).await {
            Ok(media) => {
                return insert_media(db_conn, media)
                    .map(MediaIds::new_single)
                    .map_err(FetchMediaError::DatabaseError)
            }
            Err(e) if matches!(e, MediaResolveError::FailedProcessing(_)) => {
                return Err(FetchMediaError::ResolveError(e))
            }
            Err(MediaResolveError::UnsupportedUrl) => unsupported = true,
            Err(MediaResolveError::InvalidResource) => invalid = true,
            Err(MediaResolveError::ResourceNotFound) => not_found = true,
            _ => {}
        };

        match resolve_media_list(&media_url).await {
            Ok((mut media_list, media_urls)) => {
                let mut media_ids = Vec::with_capacity(media_urls.len());
                for media_url in media_urls {
                    let id = self.fetch_media(db_conn, &media_url).await?;
                    media_ids.push(id);
                }
                media_list.media_ids = media_ids
                    .iter()
                    .map(|id| id.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
                    .into();
                return insert_media_list(db_conn, media_list)
                    .map(|id| MediaIds::new_multiple(id, MediaIdList(media_ids)))
                    .map_err(FetchMediaError::DatabaseError);
            }
            Err(e) if matches!(e, MediaResolveError::FailedProcessing(_)) => {
                return Err(FetchMediaError::ResolveError(e))
            }
            Err(MediaResolveError::UnsupportedUrl) => unsupported = true,
            Err(MediaResolveError::InvalidResource) => invalid = true,
            Err(MediaResolveError::ResourceNotFound) => not_found = true,
            _ => {}
        };

        if not_found {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::ResourceNotFound,
            ))
        } else if invalid {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::InvalidResource,
            ))
        } else if unsupported {
            Err(FetchMediaError::ResolveError(
                MediaResolveError::UnsupportedUrl,
            ))
        } else {
            unreachable!("either one of the above must be true")
        }
    }

    pub async fn _get_current_playlist(
        &self,
        db_conn: &mut SqliteConnection,
    ) -> Result<Option<Playlist>> {
        let playlist_id = *self.current_playlist.lock().await;
        playlist_id
            .map(|id| query_playlist_from_id(db_conn, id))
            .transpose()
            .map(Option::flatten)
    }

    pub async fn set_current_playlist(&self, id: Option<PlaylistId>) {
        *self.current_playlist.lock().await = id;
    }
}
