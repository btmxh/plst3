use std::{borrow::Cow, collections::HashMap, sync::Arc};

use anyhow::Context;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post, put},
    Router,
};
use sailfish::TemplateOnce;
use time::Duration;

use crate::{
    db::{
        media::{query_media_with_id, Media},
        playlist::{append_to_playlist, create_empty_playlist, query_playlist_from_id, PlaylistId},
        playlist_item::{query_playlist_item, PlaylistItem, PlaylistItemId},
    },
    resolvers::MediaResolveError,
};

use super::{
    app::{AppRouter, AppState, FetchMediaError},
    ResponseResult,
};

pub fn playlist_router() -> AppRouter {
    Router::new()
        .route("/playlist/:id/add", patch(playlist_add))
        .route("/playlist/:id/play", post(playlist_play))
        .route("/playlist/new", put(playlist_new))
        .route("/playlist/:id/list", get(playlist_get))
}

#[derive(Debug)]
enum AddPosition {
    QueueNext,
    AddToEnd,
}

impl Default for AddPosition {
    fn default() -> Self {
        Self::QueueNext
    }
}

impl AddPosition {
    pub fn from_string(string: &str) -> AddPosition {
        match string {
            "queue-next" => AddPosition::QueueNext,
            "add-to-end" => AddPosition::AddToEnd,
            _ => {
                tracing::warn!(
                    "invalid add position: {string}. Falling back to default: {:?}",
                    Self::default()
                );
                Self::default()
            }
        }
    }
}

async fn playlist_add(
    Path(playlist_id): Path<i32>,
    Query(query): Query<HashMap<String, String>>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<(StatusCode, Cow<'static, str>)> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let position = query
        .get("position")
        .map(|p| AddPosition::from_string(p))
        .unwrap_or_default();
    if let Some(url) = query.get("url") {
        match app.fetch_medias(&mut db_conn, url).await {
            Ok(medias) => {
                let media_ids = medias.media_ids();
                let playlist = query_playlist_from_id(&mut db_conn, playlist_id)
                    .context("unable to fetch current playlist")?;
                if let Some(playlist) = playlist {
                    let pivot = match position {
                        AddPosition::QueueNext => playlist.current_item,
                        AddPosition::AddToEnd => playlist.last_playlist_item,
                    }
                    .or(playlist.last_playlist_item);
                    let updated = append_to_playlist(&mut db_conn, playlist.id, pivot, media_ids)
                        .context("unable to append playlist items to playlist")?;
                    let msg = if updated {
                        "Media(s) added"
                    } else {
                        "No media was added due to empty media/media list URL"
                    }
                    .into();
                    Ok((StatusCode::OK, msg))
                } else {
                    Ok((StatusCode::NOT_FOUND, "Playlist not found".into()))
                }
            }
            Err(FetchMediaError::InvalidUrl(e)) => Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Invalid URL: {e}").into(),
            )),
            Err(FetchMediaError::DatabaseError(e)) => Err(e.into()),
            Err(FetchMediaError::ResolveError(e)) => match &e {
                MediaResolveError::FailedProcessing(_) => Err(e.into()),
                MediaResolveError::UnsupportedUrl => Ok((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("URL not supported: {url}").into(),
                )),
                MediaResolveError::InvalidResource => Err(e.into()),
                MediaResolveError::ResourceNotFound => Ok((
                    StatusCode::NOT_FOUND,
                    format!("Media not found: {e}").into(),
                )),
            },
        }
    } else {
        Ok((StatusCode::UNPROCESSABLE_ENTITY, "No url specified".into()))
    }
}

async fn playlist_play(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Cow<'static, str>> {
    app.set_current_playlist(Some(PlaylistId(playlist_id)))
        .await;
    Ok(format!("Current playlist set to playlist id {playlist_id}").into())
}

async fn playlist_new(
    Query(query): Query<HashMap<String, String>>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<(StatusCode, Cow<'static, str>)> {
    let mut db_conn = app.acquire_db_connection()?;
    let id = create_empty_playlist(
        &mut db_conn,
        query
            .get("title")
            .map(|s| s.as_str())
            .unwrap_or("<unnamed>"),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        format!("New playlist created with id {id}").into(),
    ))
}

struct Formatter;
impl Formatter {
    pub fn duration(&self, duration: &Duration) -> String {
        let hours = duration.whole_hours();
        let minutes = duration.whole_minutes();
        let seconds = duration.whole_seconds();
        format!("{:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds)
    }
}

#[derive(TemplateOnce)]
#[template(path = "playlist-get.stpl")]
struct PlaylistGetTemplate {
    current_id: Option<PlaylistItemId>,
    items: Vec<PlaylistItem>,
    medias: Vec<Option<Media>>,
    total_duration: Duration,
    fmt: Formatter,
}

async fn playlist_get(
    Path(playlist_id): Path<i32>,
    Query(query): Query<HashMap<String, String>>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    let playlist = query_playlist_from_id(&mut db_conn, PlaylistId(playlist_id))?
        .context("unable to query playlist")?;
    let item = query
        .get("after")
        .and_then(|s| {
            s.parse::<PlaylistItemId>()
                .map_err(|e| tracing::warn!("error parsing after playlist item id: {e}"))
                .ok()
        })
        .or(playlist.first_playlist_item)
        .and_then(|id| query_playlist_item(&mut db_conn, id).transpose())
        .transpose()?;
    let limit = query
        .get("limit")
        .and_then(|s| {
            s.parse::<usize>()
                .map_err(|e| tracing::warn!("error parsing limit field: {e}"))
                .ok()
        })
        .unwrap_or(10)
        .clamp(1, 30);
    let mut items = Vec::with_capacity(limit);
    if let Some(item) = item {
        items.push(item);
        while items.len() < limit
            && items
                .last()
                .and_then(|item| item.next)
                .map(|id| query_playlist_item(&mut db_conn, id))
                .transpose()?
                .flatten()
                .is_some()
        {}
    }

    let mut medias = Vec::with_capacity(items.len());
    let mut total_duration = Duration::ZERO;
    for item in items.iter() {
        let media = query_media_with_id(&mut db_conn, item.media_id)
            .context("unable to query media for playlist item")?;
        total_duration += media
            .as_ref()
            .and_then(|media| media.duration)
            .map(|duration| duration.0)
            .unwrap_or_default();
        medias.push(media);
    }

    let template_args = PlaylistGetTemplate {
        items,
        medias,
        current_id: playlist.current_item,
        total_duration,
        fmt: Formatter,
    };

    let html = template_args
        .render_once()
        .context("error rendering HTML")?;
    Ok(Html(html).into_response())
}
