use crate::db::{ResourceQueryError, ResourceType};

use super::{
    media::{DurationWrapper, MediaId},
    playlist_item::{
        insert_playlist_item, query_playlist_item, update_playlist_item_next_id,
        update_playlist_item_prev_id, NewPlaylistItem, PlaylistItemId,
    },
    ResourceQueryResult,
};
use diesel::{
    deserialize::{FromSql, FromSqlRow},
    expression::AsExpression,
    prelude::*,
    serialize::ToSql,
    sql_types::Integer,
    sqlite::Sqlite,
    ExpressionMethods, Queryable, Selectable, SelectableHelper, SqliteConnection,
};
use sailfish::runtime::Render;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};
use time::{Duration, PrimitiveDateTime};

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Hash, FromSqlRow, AsExpression, Serialize, Deserialize,
)]
#[diesel(sql_type = Integer)]
#[serde(transparent)]
pub struct PlaylistId(pub i32);

impl FromSql<Integer, Sqlite> for PlaylistId {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(Self(<i32 as FromSql<Integer, Sqlite>>::from_sql(bytes)?))
    }
}

impl ToSql<Integer, Sqlite> for PlaylistId {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        <i32 as ToSql<Integer, Sqlite>>::to_sql(&self.0, out)
    }
}

impl Display for PlaylistId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Render for PlaylistId {
    fn render(&self, b: &mut sailfish::runtime::Buffer) -> Result<(), sailfish::RenderError> {
        self.0.render(b)
    }
}

impl FromStr for PlaylistId {
    type Err = <i32 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i32>().map(Self)
    }
}

#[derive(Queryable, Selectable, Debug)]
#[diesel(table_name = crate::schema::playlists)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub first_playlist_item: Option<PlaylistItemId>,
    pub last_playlist_item: Option<PlaylistItemId>,
    pub add_timestamp: PrimitiveDateTime,
    pub current_item: Option<PlaylistItemId>,
    pub total_duration: DurationWrapper,
    pub num_items: i32,
}

pub fn query_playlist_from_id(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
) -> ResourceQueryResult<Playlist> {
    use crate::schema::playlists::dsl::*;
    let mut matches: Vec<Playlist> = playlists
        .filter(id.eq(playlist_id))
        .limit(1)
        .select(Playlist::as_select())
        .load(db_conn)?;
    if matches.is_empty() {
        Err(ResourceQueryError::ResourceNotFound(
            ResourceType::Playlist,
            playlist_id.into(),
        ))
    } else {
        Ok(matches.swap_remove(0))
    }
}

fn append_to_playlist_single(
    db_conn: &mut SqliteConnection,
    new_playlist_item: NewPlaylistItem,
) -> ResourceQueryResult<PlaylistItemId> {
    let prev = new_playlist_item.prev;
    let next = new_playlist_item.next;
    let playlist_id = new_playlist_item.playlist_id;
    let item_id = insert_playlist_item(db_conn, new_playlist_item)?;
    if let Some(prev) = prev {
        update_playlist_item_next_id(db_conn, prev, Some(item_id))?;
    } else {
        update_playlist_first_item(db_conn, playlist_id, Some(item_id))?;
    }
    if let Some(next) = next {
        update_playlist_item_prev_id(db_conn, next, Some(item_id))?;
    } else {
        update_playlist_last_item(db_conn, playlist_id, Some(item_id))?;
    }
    Ok(item_id)
}

pub fn append_to_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    prev: Option<PlaylistItemId>,
    media_ids: &[MediaId],
    total_duration: Duration,
) -> ResourceQueryResult<Vec<PlaylistItemId>> {
    let next = match prev {
        Some(id) => query_playlist_item(db_conn, id)?.next,
        None => query_playlist_from_id(db_conn, playlist_id)?.first_playlist_item,
    };
    let mut item_ids = vec![];
    for media_id in media_ids.iter().cloned() {
        item_ids.push(append_to_playlist_single(
            db_conn,
            NewPlaylistItem {
                playlist_id,
                media_id,
                prev: item_ids.last().cloned().or(prev),
                next,
            },
        )?);
    }
    if !item_ids.is_empty() {
        update_playlist(db_conn, playlist_id, total_duration, media_ids.len() as i32)?;
    }
    Ok(item_ids)
}

pub fn update_playlist_first_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(first_playlist_item.eq(item_id))
        .get_result::<Playlist>(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::Playlist, playlist_id.into())
            })
        })
}

pub fn update_playlist_last_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(last_playlist_item.eq(item_id))
        .execute(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::Playlist, playlist_id.into())
            })
        })
}

pub(crate) fn update_playlist_current_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(current_item.eq(item_id))
        .execute(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::Playlist, playlist_id.into())
            })
        })
}

pub async fn create_empty_playlist(
    db_conn: &mut SqliteConnection,
    playlist_title: &str,
) -> Result<PlaylistId, diesel::result::Error> {
    use crate::schema::playlists::dsl::*;
    diesel::insert_into(playlists)
        .values(title.eq(playlist_title))
        .returning(id)
        .get_result(db_conn)
        .map(PlaylistId)
}

pub fn update_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    add_duration: Duration,
    num_add_items: i32,
) -> ResourceQueryResult<Playlist> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set((
            total_duration.eq(total_duration + add_duration.as_seconds_f64().round() as i32),
            num_items.eq(num_items + num_add_items),
        ))
        .get_result(db_conn)
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::Playlist, playlist_id.into())
            })
        })
}

pub fn query_playlists(
    db_conn: &mut SqliteConnection,
    offset: usize,
    limit: usize,
) -> ResourceQueryResult<Box<[Playlist]>> {
    use crate::schema::playlists::dsl::*;
    Ok(playlists
        .order(add_timestamp.desc())
        .offset(offset.try_into().unwrap_or_default())
        .limit(limit.try_into().unwrap_or(10))
        .select(Playlist::as_select())
        .load(db_conn)?
        .into())
}

pub fn rename_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    new_title: &str,
) -> ResourceQueryResult<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(title.eq(new_title))
        .execute(db_conn)?;
    Ok(())
}

pub fn delete_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
) -> ResourceQueryResult<()> {
    use crate::schema::playlists::dsl::*;
    diesel::delete(playlists)
        .filter(id.eq(playlist_id))
        .execute(db_conn)?;
    Ok(())
}
