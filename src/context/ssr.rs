use std::{borrow::Cow, collections::HashSet, sync::Arc};

use super::{
    app::{AppRouter, AppState},
    ResponseError, ResponseResult,
};
use crate::db::{
    media::{query_media_with_id, Media},
    playlist::{query_playlist_from_id, query_playlists, Playlist, PlaylistId},
    playlist_item::{query_playlist_item, PlaylistItem, PlaylistItemId},
};
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use sailfish::TemplateOnce;
use serde::{de, Deserialize, Deserializer};
use time::{
    format_description::well_known::{
        iso8601::{Config, EncodedConfig, FormattedComponents, TimePrecision},
        Iso8601,
    },
    Duration, PrimitiveDateTime,
};

pub fn ssr_router() -> AppRouter {
    AppRouter::new()
        .route("/", get(index))
        .route("/index", get(index))
        .route("/watch/:id", get(watch))
        .route("/watch", get(watch_select))
        .route("/playlist/:id/list", get(playlist_get))
}

#[derive(TemplateOnce)]
#[template(path = "index.stpl")]
struct IndexTemplate;

async fn index() -> ResponseResult<Html<String>> {
    Ok(Html(IndexTemplate.render_once()?))
}

#[derive(TemplateOnce)]
#[template(path = "watch.stpl")]
struct WatchTemplate {
    pid: PlaylistId,
}

async fn watch(Path(pid): Path<i32>) -> ResponseResult<Html<String>> {
    Ok(Html(
        WatchTemplate {
            pid: PlaylistId(pid),
        }
        .render_once()?,
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

    #[allow(unused)]
    pub fn datetime(&self, datetime: &PrimitiveDateTime) -> String {
        const ENCODED_FORMAT: EncodedConfig = Config::DEFAULT
            .set_time_precision(TimePrecision::Second {
                decimal_digits: None,
            })
            .set_formatted_components(FormattedComponents::DateTime)
            .encode();
        datetime
            .format(&Iso8601::<ENCODED_FORMAT>)
            .unwrap_or_else(|_| "Invalid timestamp".into())
    }

    pub fn date(&self, datetime: &PrimitiveDateTime) -> String {
        datetime
            .format(&Iso8601::DATE)
            .unwrap_or_else(|_| "Invalid timestamp".into())
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

#[derive(Deserialize)]
struct WatchSelectParams {
    #[serde(default)]
    offset: usize,
}

#[derive(TemplateOnce)]
#[template(path = "watch_select.stpl")]
struct WatchSelectTemplate<'a> {
    playlists: &'a [Playlist],
    next_offset: Option<usize>,
    prev_offset: Option<usize>,
    formatter: Formatter,
}

async fn watch_select(
    Query(WatchSelectParams { offset }): Query<WatchSelectParams>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Html<String>> {
    let mut db_conn = app.acquire_db_connection()?;
    let count = 10;
    let playlists = query_playlists(&mut db_conn, offset, count + 1)?;
    let prev_offset = if offset > 0 {
        Some(offset.checked_sub(count).unwrap_or_default())
    } else {
        None
    };
    let next_offset = if playlists.len() > count {
        Some(offset + count)
    } else {
        None
    };
    Ok(Html(
        WatchSelectTemplate {
            playlists: &playlists[0..count.min(playlists.len())],
            prev_offset,
            next_offset,
            formatter: Formatter,
        }
        .render_once()?,
    ))
}
