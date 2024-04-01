use super::{
    app::{AppRouter, AppState, FetchMediaError},
    ResponseError, ResponseResult,
};
use crate::{
    db::{
        media::{query_media_with_id, replace_media_metadata, update_media_alt_data, MediaId},
        playlist::{
            append_to_playlist, create_empty_playlist, delete_playlist, query_playlist_from_id,
            rename_playlist, update_playlist, update_playlist_first_item,
            update_playlist_last_item, PlaylistId,
        },
        playlist_item::{
            playlist_items_with_media_id, query_playlist_item, remove_playlist_item,
            update_playlist_item_next_id, update_playlist_item_prev_and_next_id,
            update_playlist_item_prev_id, PlaylistItemId,
        },
        ResourceQueryResult,
    },
    resolvers::resolve_media,
};
use anyhow::anyhow;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    response::{AppendHeaders, IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Form, Json, Router,
};
use diesel::SqliteConnection;
use serde::Deserialize;
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::Arc,
};
use time::Duration;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use url::Url;

pub fn playlist_router() -> AppRouter {
    Router::new()
        .route("/playlist/:id/add", post(playlist_add))
        .route("/playlist/:id/play", post(playlist_play))
        .route("/playlist/new", put(playlist_new))
        .route("/playlist/:id/rename", patch(playlist_rename))
        .route("/playlist/:id/rename-norefresh", patch(playlist_rename))
        .route("/playlist/:id/next", patch(playlist_next))
        .route("/playlist/:id/prev", patch(playlist_prev))
        .route("/playlist/:id/servermedia", get(legacy_servermedia))
        .route("/servermedia/:id", get(servermedia))
        .route("/playlist/goto/:id", patch(playlist_goto))
        .route("/playlist/:id/api/current", get(playlist_current))
        .route("/playlist/:id/delete", delete(playlist_delete))
        .route("/playlist/:id/deletelist", delete(playlist_delete_list))
        .route("/media/:id/update", patch(update_media))
        .route("/media/:id/metadata/edit", patch(update_media_metadata))
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
) -> ResponseResult<()> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let PlaylistArgInfo { position, url } = info;
    let medias = app.fetch_medias(&mut db_conn, &url).await?;
    let playlist = query_playlist_from_id(&mut db_conn, playlist_id)?;
    let pivot = match position {
        AddPosition::QueueNext => playlist.current_item,
        AddPosition::AddToEnd => playlist.last_playlist_item,
    }
    .or(playlist.last_playlist_item);
    let total_duration = medias.total_duration();
    let media_ids = medias.media_ids();
    let item_ids =
        append_to_playlist(&mut db_conn, playlist.id, pivot, &media_ids, total_duration)?;
    #[allow(unused)]
    if let Some(first_item_id) = item_ids.first() {
        #[cfg(feature = "notifications")]
        app.notify_playlist_add(&playlist, &medias, *first_item_id);
        app.refresh_playlist(playlist.id).await;
    }
    Ok(())
}

async fn playlist_play(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<impl IntoResponse> {
    app.set_current_playlist(Some(PlaylistId(playlist_id)))
        .await?;
    Ok(AppendHeaders([("HX-Refresh", "true")]))
}

#[derive(Deserialize)]
struct PlaylistTitle {
    title: Option<String>,
    #[serde(default)]
    refresh: bool,
}

fn redirect(path: &str) -> Response {
    AppendHeaders([("HX-Redirect", path)]).into_response()
}

async fn playlist_new(
    header: HeaderMap,
    Query(PlaylistTitle { title, refresh }): Query<PlaylistTitle>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<impl IntoResponse> {
    let mut db_conn = app.acquire_db_connection()?;
    let title = title
        .as_deref()
        .or_else(|| header.get("HX-Prompt").and_then(|v| v.to_str().ok()))
        .unwrap_or("<unnamed>");
    let id = create_empty_playlist(&mut db_conn, title).await?;
    let mut headers = Vec::<(&'static str, String)>::new();
    if refresh {
        headers.push(("HX-Refresh", "true".into()));
    } else {
        headers.push(("HX-Redirect", format!("/watch/{id}")));
    }
    Ok(AppendHeaders(headers))
}

async fn playlist_rename(
    header: HeaderMap,
    Path(playlist_id): Path<i32>,
    Query(PlaylistTitle { title, refresh }): Query<PlaylistTitle>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<impl IntoResponse> {
    let mut db_conn = app.acquire_db_connection()?;
    let title = title
        .as_deref()
        .or_else(|| header.get("HX-Prompt").and_then(|v| v.to_str().ok()))
        .unwrap_or("<unnamed>");
    rename_playlist(&mut db_conn, PlaylistId(playlist_id), title)?;
    let mut headers = Vec::<(&'static str, String)>::new();
    headers.push(("HX-Trigger", "metadata-changed".into()));
    if refresh {
        headers.push(("HX-Refresh", "true".into()));
    }
    Ok(AppendHeaders(headers))
}

async fn playlist_delete_list(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    delete_playlist(&mut db_conn, PlaylistId(playlist_id))?;
    Ok(redirect("/watch"))
}

async fn playlist_next(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    app.next(&mut db_conn, PlaylistId(playlist_id)).await?;
    Ok("a".into_response())
}

async fn playlist_prev(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    app.prev(&mut db_conn, PlaylistId(playlist_id)).await?;
    Ok("a".into_response())
}

async fn legacy_servermedia(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    request: Request<Body>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(media) = AppState::get_current_media(&mut db_conn, playlist_id).await? {
        if media.media_type == "local" {
            let path = Url::parse(&media.url)
                .map_err(|e| anyhow!("Invalid URL: {e}"))?
                .to_file_path()
                .map_err(|_| anyhow!("Unable to convert local URL to path"))?;
            tracing::info!("transfering file: {}", path.display());
            return Ok(ServeFile::new(path).oneshot(request).await?.into_response());
        }
    }

    Ok((StatusCode::NOT_FOUND, "Playlist not found").into_response())
}

// this is basically an arbitrary file read XDD
async fn servermedia(
    Path(media_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    request: Request<Body>,
) -> ResponseResult<Response> {
    let media_id = MediaId(media_id);
    let mut db_conn = app.acquire_db_connection()?;
    let media = query_media_with_id(&mut db_conn, media_id)?;
    if media.media_type == "local" {
        let path = Url::parse(&media.url)
            .map_err(|e| anyhow!("Invalid URL: {e}"))?
            .to_file_path()
            .map_err(|_| anyhow!("Unable to convert local URL to path"))?;
        tracing::info!("transfering file: {}", path.display());
        return Ok(ServeFile::new(path).oneshot(request).await?.into_response());
    }

    Ok((StatusCode::NOT_FOUND, "Media not found").into_response())
}

async fn playlist_goto(
    Path(item_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<&'static str> {
    let item_id = PlaylistItemId(item_id);
    let mut db_conn = app.acquire_db_connection()?;
    app.set_playlist_item_as_current(&mut db_conn, None, item_id)
        .await?;
    Ok("goto successfully")
}

async fn playlist_current(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    if let Some(media) = AppState::get_current_media(&mut db_conn, playlist_id).await? {
        Ok(Json(media).into_response())
    } else {
        Ok(Json(serde_json::Value::Null).into_response())
    }
}

async fn playlist_delete(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    Form(ids): Form<HashMap<String, String>>,
) -> ResponseResult<Response> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let ids = ids
        .keys()
        .filter_map(|key| key.strip_prefix("playlist-item-"))
        .filter_map(|id| id.parse::<PlaylistItemId>().ok())
        .collect::<Box<_>>();
    let mut media_changed = false;
    for id in &*ids {
        media_changed |= remove_playlist_item(&mut db_conn, *id)?;
    }

    app.refresh_playlist(playlist_id).await;
    if media_changed {
        app.media_changed(playlist_id, None).await?;
    }

    Ok(().into_response())
}

async fn update_media(
    Path(media_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<impl IntoResponse> {
    let mut db_conn = app.acquire_db_connection()?;
    let media = query_media_with_id(&mut db_conn, MediaId(media_id))?;
    let resolved_media = resolve_media(
        &Url::parse(&media.url)
            .map_err(|e| ResponseError::Generic(anyhow!("unable to parse url of media: {e}")))?,
        Some(media.media_type.as_str()),
    )
    .await
    .map_err(FetchMediaError::ResolveError)?;
    let delta_duration_per_media = resolved_media
        .duration
        .map(|d| Duration::seconds_f64(d as f64))
        .unwrap_or_default()
        - media.duration.map(|d| d.0).unwrap_or_default();
    replace_media_metadata(&mut db_conn, media.id, resolved_media)?;
    let items = playlist_items_with_media_id(&mut db_conn, media.id)?;
    let mut playlists = HashMap::<PlaylistId, i32>::new();
    for item in items.iter() {
        match playlists.entry(item.playlist_id) {
            Entry::Occupied(mut v) => *v.get_mut() += 1,
            Entry::Vacant(v) => {
                v.insert(1);
            }
        };
    }

    for (playlist_id, num_occurences) in playlists.into_iter() {
        update_playlist(
            &mut db_conn,
            playlist_id,
            delta_duration_per_media * num_occurences,
            0,
        )?;
        app.refresh_playlist(playlist_id).await;
        app.metadata_changed(playlist_id).await;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct MediaMetadata {
    media_title: String,
    media_artist: String,
}

async fn update_media_metadata(
    Path(media_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    Form(MediaMetadata {
        media_title,
        media_artist,
    }): Form<MediaMetadata>,
) -> ResponseResult<impl IntoResponse> {
    let media_id = MediaId(media_id);
    let mut db_conn = app.acquire_db_connection()?;
    update_media_alt_data(
        &mut db_conn,
        media_id,
        media_title.as_str(),
        media_artist.as_str(),
    )?;
    let items = playlist_items_with_media_id(&mut db_conn, media_id)?;
    let playlists: HashSet<PlaylistId> = items.iter().map(|item| item.playlist_id).collect();
    for playlist_id in playlists {
        app.refresh_playlist(playlist_id).await;
        app.metadata_changed(playlist_id).await;
        #[cfg(feature = "media-controls")]
        if app.get_current_playlist().await == Some(playlist_id) {
            app.update_media_metadata(true).await?;
        }
    }
    Ok(())
}
