use super::{
    app::{AppRouter, AppState, FetchMediaError},
    ResponseResult,
};
use crate::{
    db::{
        media::{query_media_with_id, Media, MediaId},
        playlist::{append_to_playlist, create_empty_playlist, query_playlist_from_id, PlaylistId},
        playlist_item::{
            query_playlist_item, set_playlist_item_as_current, PlaylistItem, PlaylistItemId,
        },
    },
    resolvers::MediaResolveError,
};
use anyhow::{anyhow, Context};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post, put},
    Form, Json, Router,
};
use sailfish::TemplateOnce;
use serde::Deserialize;
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use time::Duration;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use url::Url;

pub fn playlist_router() -> AppRouter {
    Router::new()
        .route("/playlist/:id/add", post(playlist_add))
        .route("/playlist/:id/play", post(playlist_play))
        .route("/playlist/new", put(playlist_new))
        .route("/playlist/:id/list", get(playlist_get))
        .route("/playlist/:id/next", patch(playlist_next))
        .route("/playlist/:id/prev", patch(playlist_prev))
        .route("/playlist/:id/servermedia", get(legacy_servermedia))
        .route("/servermedia/:id", get(servermedia))
        .route("/playlist/goto/:id", patch(playlist_goto))
        .route("/playlist/:id/api/current", get(playlist_current))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AddPosition {
    QueueNext,
    AddToEnd,
}

impl Default for AddPosition {
    fn default() -> Self {
        Self::QueueNext
    }
}

#[derive(Deserialize)]
struct PlaylistArgInfo {
    position: AddPosition,
    url: String,
}

async fn playlist_add(
    State(app): State<Arc<AppState>>,
    Path(playlist_id): Path<i32>,
    Form(info): Form<PlaylistArgInfo>,
) -> ResponseResult<(StatusCode, Cow<'static, str>)> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let PlaylistArgInfo { position, url } = info;
    match app.fetch_medias(&mut db_conn, &url).await {
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
                app.refresh_playlist(playlist.id).await;
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
        let minutes = duration.whole_minutes() % 60;
        let seconds = duration.whole_seconds() % 60;
        format!("{:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds)
    }
}

#[derive(TemplateOnce)]
#[template(path = "playlist-get.stpl")]
struct PlaylistGetTemplate {
    // pid: PlaylistId,
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
        .unwrap_or(1000)
        .clamp(1, 3000);
    let mut items = Vec::with_capacity(limit);
    if let Some(item) = item {
        items.push(item);
        while items.len() < limit {
            if let Some(item) = items
                .last()
                .and_then(|item| item.next)
                .map(|id| query_playlist_item(&mut db_conn, id))
                .transpose()?
                .flatten()
            {
                items.push(item);
            } else {
                break;
            }
        }
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
        // pid: PlaylistId(playlist_id),
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

async fn playlist_next(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    app.next(PlaylistId(playlist_id)).await?;
    Ok("a".into_response())
}

async fn playlist_prev(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(item) = AppState::get_current_item(&mut db_conn, playlist_id)
        .await
        .context("unable to get current item of playlist")?
        .and_then(|item| item.prev)
    {
        set_playlist_item_as_current(&app, &mut db_conn, item).await?;
        return Ok("a".into_response());
    }

    if let Some(item) = query_playlist_from_id(&mut db_conn, playlist_id)
        .context("unable to query playlist")?
        .and_then(|p| p.last_playlist_item)
    {
        set_playlist_item_as_current(&app, &mut db_conn, item).await?;
        return Ok("a".into_response());
    }

    Ok((StatusCode::NO_CONTENT, "").into_response())
}

async fn legacy_servermedia(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    request: Request<Body>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(media) = AppState::get_current_media(&mut db_conn, playlist_id)
        .await
        .context("unable to query current media")?
    {
        if media.media_type == "local" {
            let path = Url::parse(&media.url)
                .context("invalid url")?
                .to_file_path()
                .map_err(|_| anyhow!("invalid file path"))?;
            tracing::info!("transfering file: {}", path.display());
            return Ok(ServeFile::new(path).oneshot(request).await?.into_response());
        }
    }

    Ok((StatusCode::NOT_FOUND, "Playlist not found").into_response())
}

async fn servermedia(
    Path(media_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    request: Request<Body>,
) -> ResponseResult<Response> {
    let media_id = MediaId(media_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(media) =
        query_media_with_id(&mut db_conn, media_id).context("unable to query server media")?
    {
        if media.media_type == "local" {
            let path = Url::parse(&media.url)
                .context("invalid url")?
                .to_file_path()
                .map_err(|_| anyhow!("invalid file path"))?;
            tracing::info!("transfering file: {}", path.display());
            return Ok(ServeFile::new(path).oneshot(request).await?.into_response());
        }
    }

    Ok((StatusCode::NOT_FOUND, "Media not found").into_response())
}

async fn playlist_goto(
    Path(item_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<&'static str> {
    let item_id = PlaylistItemId(item_id);
    let mut db_conn = app.acquire_db_connection()?;
    set_playlist_item_as_current(&app, &mut db_conn, item_id).await?;
    Ok("goto successfully")
}

async fn playlist_current(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(media) = AppState::get_current_media(&mut db_conn, playlist_id)
        .await
        .context("unable to query current media")?
    {
        Ok(Json(media).into_response())
    } else {
        Ok((StatusCode::NOT_FOUND, "not found").into_response())
    }
}
