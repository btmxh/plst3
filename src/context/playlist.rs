use super::{
    app::{AppRouter, AppState},
    ResponseError, ResponseResult,
};
use crate::db::{
    media::{query_media_with_id, Media, MediaId},
    playlist::{
        append_to_playlist, create_empty_playlist, query_playlist_from_id,
        update_playlist_first_item, update_playlist_last_item, PlaylistId,
    },
    playlist_item::{
        query_playlist_item, remove_playlist_item, update_playlist_item_next_id,
        update_playlist_item_prev_and_next_id, update_playlist_item_prev_id, PlaylistItem,
        PlaylistItemId,
    },
    ResourceQueryResult,
};
use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Form, Json, Router,
};
use diesel::SqliteConnection;
use sailfish::TemplateOnce;
use serde::{de, Deserialize, Deserializer};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
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
        .route("/playlist/:id/list", get(playlist_get))
        .route("/playlist/:id/next", patch(playlist_next))
        .route("/playlist/:id/prev", patch(playlist_prev))
        .route("/playlist/:id/servermedia", get(legacy_servermedia))
        .route("/servermedia/:id", get(servermedia))
        .route("/playlist/goto/:id", patch(playlist_goto))
        .route("/playlist/:id/api/current", get(playlist_current))
        .route("/playlist/:id/delete", delete(playlist_delete))
        .route("/playlist/:id/up", patch(playlist_move_up))
        .route("/playlist/:id/down", patch(playlist_move_down))
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
        app.notify_playlist_add(playlist_id, &medias, *first_item_id);
        app.refresh_playlist(playlist.id).await;
    }
    Ok(())
}

async fn playlist_play(
    #[cfg(feature = "media-controls")] Path(playlist_id): Path<i32>,
    #[cfg(feature = "media-controls")] State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    #[cfg(feature = "media-controls")]
    {
        app.set_current_playlist(Some(PlaylistId(playlist_id)))
            .await;
        Ok(format!("Current playlist set to playlist id {playlist_id}").into_response())
    }

    #[cfg(not(feature = "media-controls"))]
    {
        Ok(StatusCode::METHOD_NOT_ALLOWED.into_response())
    }
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
    medias: Vec<Media>,
    total_duration: Duration,
    total_clients: usize,
    fmt: Formatter,
    ids: HashSet<PlaylistItemId>,
}

struct PlaylistGetArgs {
    from: Option<PlaylistItemId>,
    limit: usize,
    ids: HashSet<PlaylistItemId>,
}

impl<'de> Deserialize<'de> for PlaylistGetArgs {
    fn deserialize<D>(deserializer: D) -> std::prelude::v1::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PlaylistGetArgsVisitor)
    }
}

struct PlaylistGetArgsVisitor;
impl<'de> de::Visitor<'de> for PlaylistGetArgsVisitor {
    type Value = PlaylistGetArgs;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a PlaylistGetArgs")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut args = PlaylistGetArgs {
            from: None,
            limit: 1000,
            ids: HashSet::new(),
        };
        while let Some(key) = map.next_key::<Cow<'static, str>>()? {
            if key == "from" {
                args.from = Some(map.next_value()?);
            } else if key == "limit" {
                args.limit = map.next_value::<usize>()?.clamp(1, 3000);
            } else if let Some(id) = key.strip_prefix("playlist-item-") {
                if let Ok(id) = id.parse() {
                    args.ids.insert(id);
                }
            }
        }

        Ok(args)
    }
}

async fn playlist_get(
    Path(playlist_id): Path<i32>,
    Query(PlaylistGetArgs { from, limit, ids }): Query<PlaylistGetArgs>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    let playlist_id = PlaylistId(playlist_id);
    let playlist = query_playlist_from_id(&mut db_conn, playlist_id)?;
    let from_id = from.or(playlist.first_playlist_item);
    let mut items = Vec::with_capacity(limit);
    if let Some(from_id) = from_id {
        let from = query_playlist_item(&mut db_conn, from_id)?;
        if from.playlist_id != playlist_id {
            return Err(ResponseError::InvalidRequest(
                "Playlist item does not belong to current playlist".into(),
            ));
        }

        items.push(from);
        while items.len() < limit {
            if let Some(next_id) = items.last().unwrap().next {
                let next = query_playlist_item(&mut db_conn, next_id)?;
                items.push(next);
            } else {
                break;
            }
        }
    }

    let mut medias = Vec::with_capacity(items.len());
    for item in items.iter() {
        let media = query_media_with_id(&mut db_conn, item.media_id)?;
        medias.push(media);
    }

    let template_args = PlaylistGetTemplate {
        items,
        medias,
        current_id: playlist.current_item,
        total_duration: playlist.total_duration.0,
        total_clients: app.get_num_clients(playlist.id).await,
        fmt: Formatter,
        ids,
    };

    let html = template_args.render_once()?;
    Ok(Html(html).into_response())
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

#[derive(Clone, Debug)]
struct PlaylistItemRange {
    first: PlaylistItemId,
    last: PlaylistItemId,
}

fn partition_ids_into_ranges(
    db_conn: &mut SqliteConnection,
    ids: HashMap<String, String>,
) -> ResourceQueryResult<Vec<PlaylistItemRange>> {
    let ids = ids
        .keys()
        .filter_map(|key| key.strip_prefix("playlist-item-"))
        .filter_map(|id| id.parse::<PlaylistItemId>().ok())
        .collect::<Box<[_]>>();
    let mut range_dict = HashMap::new();
    let mut items = Vec::new();
    for id in &*ids {
        let item = query_playlist_item(db_conn, *id)?;
        range_dict.insert(
            *id,
            PlaylistItemRange {
                first: *id,
                last: *id,
            },
        );
        items.push(item);
    }

    for item in items {
        if let Some(prev_item) = item.prev.as_ref() {
            let prev_range = range_dict.get(prev_item);
            let cur_range = range_dict.get(&item.id);

            if let Some((prev_range, cur_range)) = prev_range.zip(cur_range) {
                // merge prev_range and cur_range
                let merged_range = PlaylistItemRange {
                    first: prev_range.first,
                    last: cur_range.last,
                };

                range_dict.remove(prev_item);
                range_dict.remove(&item.id);
                range_dict.insert(merged_range.first, merged_range.clone());
                range_dict.insert(merged_range.last, merged_range.clone());
            }
        }
    }

    let ranges = range_dict
        .into_iter()
        .filter(|(id, range)| *id == range.first)
        .map(|(_, range)| range)
        .collect();
    Ok(ranges)
}

async fn playlist_move_up(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    Form(ids): Form<HashMap<String, String>>,
) -> ResponseResult<()> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let ranges = partition_ids_into_ranges(&mut db_conn, ids)?;
    for range in ranges {
        tracing::info!("{range:?}");
        let PlaylistItemRange { first, last } = range;
        let prev = query_playlist_item(&mut db_conn, first)?.prev;
        let next = query_playlist_item(&mut db_conn, last)?.next;
        if let Some(next) = next {
            let next_next = query_playlist_item(&mut db_conn, next)?.next;
            update_playlist_item_prev_and_next_id(&mut db_conn, next, prev, Some(first))?;
            update_playlist_item_prev_id(&mut db_conn, first, Some(next))?;
            update_playlist_item_next_id(&mut db_conn, last, next_next)?;
            if let Some(next_next) = next_next {
                update_playlist_item_prev_id(&mut db_conn, next_next, Some(last))?;
            } else {
                update_playlist_last_item(&mut db_conn, playlist_id, Some(last))?;
            }
            if let Some(prev) = prev {
                update_playlist_item_next_id(&mut db_conn, prev, Some(next))?;
            } else {
                update_playlist_first_item(&mut db_conn, playlist_id, Some(next))?;
            }
        }
    }
    app.refresh_playlist(playlist_id).await;
    Ok(())
}

async fn playlist_move_down(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    Form(ids): Form<HashMap<String, String>>,
) -> ResponseResult<()> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let ranges = partition_ids_into_ranges(&mut db_conn, ids)?;
    for range in ranges {
        let PlaylistItemRange { first, last } = range;
        let prev = query_playlist_item(&mut db_conn, first)?.prev;
        let next = query_playlist_item(&mut db_conn, last)?.next;
        if let Some(prev) = prev {
            let prev_prev = query_playlist_item(&mut db_conn, prev)?.prev;
            update_playlist_item_prev_and_next_id(&mut db_conn, prev, Some(last), next)?;
            update_playlist_item_next_id(&mut db_conn, last, Some(prev))?;
            update_playlist_item_prev_id(&mut db_conn, first, prev_prev)?;
            if let Some(prev_prev) = prev_prev {
                update_playlist_item_next_id(&mut db_conn, prev_prev, Some(first))?;
            } else {
                update_playlist_first_item(&mut db_conn, playlist_id, Some(first))?;
            }
            if let Some(next) = next {
                update_playlist_item_prev_id(&mut db_conn, next, Some(prev))?;
            } else {
                update_playlist_last_item(&mut db_conn, playlist_id, Some(prev))?;
            }
        }
    }
    app.refresh_playlist(playlist_id).await;
    Ok(())
}
