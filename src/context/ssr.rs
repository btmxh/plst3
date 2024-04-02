use std::{
    borrow::Cow,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use super::{
    app::{AppRouter, AppState},
    ResponseResult,
};
use crate::db::{
    media::{query_media_with_id, Media},
    playlist::{
        query_playlist_from_id, query_playlists, update_playlist_first_item,
        update_playlist_last_item, Playlist, PlaylistId,
    },
    playlist_item::{
        query_playlist_item, update_playlist_item_next_id, update_playlist_item_prev_and_next_id,
        update_playlist_item_prev_id, PlaylistItem, PlaylistItemId,
    },
    ResourceQueryResult,
};
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Response},
    routing::{get, patch},
    Form,
};
use diesel::SqliteConnection;
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
        .route("/playlist/:id/controller", get(playlist_controller))
        .route("/playlist/:id/up", patch(playlist_move_up))
        .route("/playlist/:id/down", patch(playlist_move_down))
        .route("/playlist/:id/listcurrent", get(playlist_listcurrent))
}

#[derive(TemplateOnce)]
#[template(path = "index.stpl")]
struct IndexTemplate {
    title: &'static str,
}

async fn index() -> ResponseResult<Html<String>> {
    Ok(Html(IndexTemplate { title: "plst3" }.render_once()?))
}

#[derive(TemplateOnce)]
#[template(path = "watch.stpl")]
struct WatchTemplate {
    pid: PlaylistId,
    title: String,
}

async fn watch(
    Path(pid): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Html<String>> {
    let mut db_conn = app.acquire_db_connection()?;
    let title = query_playlist_from_id(&mut db_conn, PlaylistId(pid))?.title;
    Ok(Html(
        WatchTemplate {
            pid: PlaylistId(pid),
            title: format!("{title} - plst3"),
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

    #[allow(unused)]
    pub fn date(&self, datetime: &PrimitiveDateTime) -> String {
        datetime
            .format(&Iso8601::DATE)
            .unwrap_or_else(|_| "Invalid timestamp".into())
    }
}

#[derive(TemplateOnce)]
#[template(path = "playlist-get.stpl")]
struct PlaylistGetTemplate {
    pid: PlaylistId,
    index_offset: usize,
    count: isize,
    args: String,
    args_json: String,
    next_args: Option<String>,
    prev_args: Option<String>,
    current_id: Option<PlaylistItemId>,
    items: Vec<PlaylistItem>,
    medias: Vec<Media>,
    total_duration: Duration,
    total_clients: usize,
    ids: HashSet<PlaylistItemId>,
    fmt: Formatter,
}

struct PlaylistGetArgs {
    base: Option<PlaylistItemId>,
    from: isize,
    to: isize,
    count: isize,
    index_offset: usize,
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
        let mut base: Option<PlaylistItemId> = None;
        let mut from: Option<isize> = None;
        let mut to: Option<isize> = None;
        let mut count: Option<isize> = None;
        let mut index_offset = 0;
        let mut ids = HashSet::new();
        while let Some(key) = map.next_key::<Cow<'static, str>>()? {
            if key == "from" {
                from = map.next_value()?;
            } else if key == "to" {
                to = Some(map.next_value()?);
            } else if key == "count" {
                count = Some(map.next_value()?);
            } else if key == "base" {
                base = Some(map.next_value()?);
            } else if key == "index_offset" {
                index_offset = map.next_value()?;
            } else if let Some(id) = key.strip_prefix("playlist-item-") {
                if let Ok(id) = id.parse() {
                    ids.insert(id);
                }
            }
        }

        let from = from
            .or(to.zip(count).map(|(to, count)| to - count))
            .unwrap_or_default();
        let count = count.unwrap_or(100);
        let to = to.unwrap_or(count + from);

        Ok(PlaylistGetArgs {
            base,
            from,
            to,
            count,
            index_offset,
            ids,
        })
    }
}

fn query_playlist_items(
    db_conn: &mut SqliteConnection,
    base: PlaylistItemId,
    from: isize,
    to: isize,
    index_offset: &mut usize,
) -> ResourceQueryResult<Vec<PlaylistItem>> {
    let mut items = VecDeque::with_capacity((to - from).try_into().expect("int cast panic"));
    let item = query_playlist_item(db_conn, base)?;
    if from <= 0 && to > 0 {
        items.push_back(item);
    }

    let mut prev_id_opt = item.prev;
    for i in from..=-1 {
        let Some(prev_id) = prev_id_opt else {
            break;
        };
        let prev = query_playlist_item(db_conn, prev_id)?;
        prev_id_opt = prev.prev;
        if i < to {
            items.push_front(prev);
            *index_offset = index_offset.saturating_sub(1);
        }
    }

    let mut next_id_opt = item.next;
    for i in 1..to {
        let Some(next_id) = next_id_opt else {
            break;
        };
        let next = query_playlist_item(db_conn, next_id)?;
        next_id_opt = next.next;
        if i >= from {
            items.push_back(next);
        }
    }

    Ok(items.into())
}

async fn playlist_get_inner(
    playlist_id: PlaylistId,
    PlaylistGetArgs {
        base,
        from,
        to,
        count,
        mut index_offset,
        ids,
    }: PlaylistGetArgs,
    app: Arc<AppState>,
) -> ResponseResult<Response> {
    let mut db_conn = app.acquire_db_connection()?;
    let playlist = query_playlist_from_id(&mut db_conn, playlist_id)?;
    let items = match base.or(playlist.first_playlist_item) {
        Some(base) => query_playlist_items(&mut db_conn, base, from, to, &mut index_offset)?,
        None => vec![],
    };
    let mut medias = Vec::with_capacity(items.len());
    for item in items.iter() {
        let media = query_media_with_id(&mut db_conn, item.media_id)?;
        medias.push(media);
    }

    let args = items
        .first()
        .map(|item| item.id)
        .map(|id| format!("base={id}&from=0&index_offset={index_offset}"))
        .unwrap_or_else(|| format!("from=0&index_offset={index_offset}"));
    let prev_args = items.first().and_then(|item| item.prev).map(|prev_base| {
        let index_offset = index_offset.saturating_sub(1);
        format!("base={prev_base}&to=1&index_offset={index_offset}")
    });
    let next_args = items.last().and_then(|item| item.next).map(|next_base| {
        let index_offset = index_offset.saturating_add(items.len());
        format!("base={next_base}&from=0&index_offset={index_offset}")
    });

    let template_args = PlaylistGetTemplate {
        pid: playlist_id,
        index_offset,
        count,
        args,
        args_json: serde_json::to_string(&serde_json::json!({
            "base": base,
            "from": 0,
            "index_offset": index_offset,
        }))
        .expect("should be valid json"),
        next_args,
        prev_args,
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

async fn playlist_get(
    Path(playlist_id): Path<i32>,
    Query(args): Query<PlaylistGetArgs>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Response> {
    playlist_get_inner(PlaylistId(playlist_id), args, app).await
}

#[derive(Deserialize)]
struct WatchSelectParams {
    #[serde(default)]
    offset: usize,
}

#[derive(TemplateOnce)]
#[template(path = "watch_select.stpl")]
struct WatchSelectTemplate<'a> {
    title: &'static str,
    playlists: &'a [(Playlist, Option<(PlaylistItem, Media)>)],
    current_id: Option<PlaylistId>,
    next_offset: Option<usize>,
    prev_offset: Option<usize>,
    formatter: Formatter,
}

#[allow(clippy::type_complexity)]
fn query_playlists_with_current_items(
    db_conn: &mut SqliteConnection,
    offset: usize,
    limit: usize,
) -> ResourceQueryResult<Vec<(Playlist, Option<(PlaylistItem, Media)>)>> {
    let playlists = query_playlists(db_conn, offset, limit)?;
    let mut result = Vec::new();
    for playlist in playlists {
        let current_item = match playlist.current_item.as_ref() {
            Some(item_id) => {
                let item = query_playlist_item(db_conn, *item_id)?;
                let media = query_media_with_id(db_conn, item.media_id)?;
                Some((item, media))
            }
            None => None,
        };
        result.push((playlist, current_item));
    }
    Ok(result)
}

async fn watch_select(
    Query(WatchSelectParams { offset }): Query<WatchSelectParams>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Html<String>> {
    let mut db_conn = app.acquire_db_connection()?;
    let count = 5;
    let playlists = query_playlists_with_current_items(&mut db_conn, offset, count + 1)?;
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
            title: "plst3",
            playlists: &playlists[0..count.min(playlists.len())],
            current_id: app.get_current_playlist().await,
            prev_offset,
            next_offset,
            formatter: Formatter,
        }
        .render_once()?,
    ))
}

#[derive(TemplateOnce)]
#[template(path = "controller.stpl")]
struct ControllerTemplate {
    pid: PlaylistId,
    playlist: Playlist,
    media_item: Option<(Media, PlaylistItem)>,
    fmt: Formatter,
}

async fn playlist_controller(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
) -> ResponseResult<Html<String>> {
    let mut db_conn = app.acquire_db_connection()?;
    let playlist = query_playlist_from_id(&mut db_conn, PlaylistId(playlist_id))?;
    let media_item = match playlist.current_item {
        Some(item_id) => {
            let item = query_playlist_item(&mut db_conn, item_id)?;
            let media = query_media_with_id(&mut db_conn, item.media_id)?;
            Some((media, item))
        }
        None => None,
    };
    Ok(Html(
        ControllerTemplate {
            pid: PlaylistId(playlist_id),
            playlist,
            media_item,
            fmt: Formatter,
        }
        .render_once()?,
    ))
}

#[derive(Clone, Debug)]
struct PlaylistItemRange {
    first: PlaylistItemId,
    last: PlaylistItemId,
    contains_base: bool,
}

fn partition_ids_into_ranges(
    db_conn: &mut SqliteConnection,
    ids: &HashSet<PlaylistItemId>,
    base: Option<PlaylistItemId>,
) -> ResourceQueryResult<Vec<PlaylistItemRange>> {
    let mut range_dict = HashMap::new();
    let mut items = Vec::new();
    for id in ids.iter().cloned() {
        let item = query_playlist_item(db_conn, id)?;
        range_dict.insert(
            id,
            PlaylistItemRange {
                first: id,
                last: id,
                contains_base: base == Some(id),
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
                    contains_base: prev_range.contains_base || cur_range.contains_base,
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
    Form(mut args): Form<PlaylistGetArgs>,
) -> ResponseResult<impl IntoResponse> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let ranges = partition_ids_into_ranges(&mut db_conn, &args.ids, args.base)?;
    for range in ranges {
        let PlaylistItemRange {
            first,
            last,
            contains_base,
        } = range;
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
            if contains_base {
                args.base = query_playlist_item(
                    &mut db_conn,
                    args.base
                        .expect("should not be None if contains_base is true"),
                )?
                .prev;
            } else if args.base == Some(next) {
                args.base = Some(range.last);
            }
        }
    }
    // app.refresh_playlist(playlist_id).await;
    Ok(playlist_get_inner(playlist_id, args, app).await)
}

async fn playlist_move_down(
    Path(playlist_id): Path<i32>,
    State(app): State<Arc<AppState>>,
    Form(mut args): Form<PlaylistGetArgs>,
) -> ResponseResult<impl IntoResponse> {
    let playlist_id = PlaylistId(playlist_id);
    let mut db_conn = app.acquire_db_connection()?;
    let ranges = partition_ids_into_ranges(&mut db_conn, &args.ids, args.base)?;
    for range in ranges {
        let PlaylistItemRange {
            first,
            last,
            contains_base,
        } = range;
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
            if contains_base {
                args.base = query_playlist_item(
                    &mut db_conn,
                    args.base
                        .expect("should not be None if contains_base is true"),
                )?
                .next;
            } else {
                args.base = Some(range.first);
            }
        }
    }
    // app.refresh_playlist(playlist_id).await;
    Ok(playlist_get_inner(playlist_id, args, app).await)
}

async fn playlist_listcurrent(
    Path(playlist_id): Path<PlaylistId>,
    State(app): State<Arc<AppState>>,
    Form(mut args): Form<PlaylistGetArgs>,
) -> ResponseResult<impl IntoResponse> {
    let mut db_conn = app.acquire_db_connection()?;
    let playlist = query_playlist_from_id(&mut db_conn, playlist_id)?;
    let current_item_index = match playlist.current_item.zip(playlist.first_playlist_item) {
        Some((current, mut first)) => {
            let mut index: isize = 0;
            while first != current {
                index += 1;
                if let Some(next) = query_playlist_item(&mut db_conn, first)?.next {
                    first = next;
                } else {
                    index = 0;
                    break;
                }
            }
            index
        }
        None => 0,
    };

    args.base = playlist.current_item;
    args.from = current_item_index / args.count * args.count - current_item_index;
    args.to = args.from + args.count;
    args.index_offset = current_item_index.try_into().expect("overflow");
    Ok(playlist_get_inner(playlist_id, args, app).await)
}
