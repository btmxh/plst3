use std::{fmt::Display, str::FromStr};

use anyhow::Result;
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
use time::PrimitiveDateTime;

use crate::db::{ResourceQueryError, ResourceType};

use super::{
    media::{query_media_with_id, MediaId},
    playlist::{
        update_playlist, update_playlist_current_item, update_playlist_first_item,
        update_playlist_last_item, PlaylistId,
    },
    ResourceQueryResult,
};

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Hash, FromSqlRow, AsExpression, Serialize, Deserialize,
)]
#[diesel(sql_type = Integer)]
#[serde(transparent)]
pub struct PlaylistItemId(pub i32);

impl FromSql<Integer, Sqlite> for PlaylistItemId {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(Self(<i32 as FromSql<Integer, Sqlite>>::from_sql(bytes)?))
    }
}

impl ToSql<Integer, Sqlite> for PlaylistItemId {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        <i32 as ToSql<Integer, Sqlite>>::to_sql(&self.0, out)
    }
}

impl FromStr for PlaylistItemId {
    type Err = <i32 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i32>().map(Self)
    }
}

impl Display for PlaylistItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Render for PlaylistItemId {
    fn render(&self, b: &mut sailfish::runtime::Buffer) -> Result<(), sailfish::RenderError> {
        self.0.render(b)
    }
}

#[derive(Queryable, Selectable, Debug)]
#[diesel(table_name = crate::schema::playlist_items)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct PlaylistItem {
    pub id: PlaylistItemId,
    pub playlist_id: PlaylistId,
    pub media_id: MediaId,
    pub prev: Option<PlaylistItemId>,
    pub next: Option<PlaylistItemId>,
    pub add_timestamp: PrimitiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::playlist_items)]
pub struct NewPlaylistItem {
    pub playlist_id: PlaylistId,
    pub media_id: MediaId,
    pub prev: Option<PlaylistItemId>,
    pub next: Option<PlaylistItemId>,
}

pub fn query_playlist_item(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
) -> ResourceQueryResult<PlaylistItem> {
    use crate::schema::playlist_items::dsl::*;
    let mut matches: Vec<PlaylistItem> = playlist_items
        .filter(id.eq(item_id))
        .limit(1)
        .select(PlaylistItem::as_select())
        .load(db_conn)?;
    if matches.is_empty() {
        Err(ResourceQueryError::ResourceNotFound(
            ResourceType::PlaylistItem,
            item_id.into(),
        ))
    } else {
        Ok(matches.swap_remove(0))
    }
}

pub fn insert_playlist_item(
    db_conn: &mut SqliteConnection,
    item: NewPlaylistItem,
) -> Result<PlaylistItemId, diesel::result::Error> {
    use crate::schema::playlist_items::dsl::*;
    diesel::insert_into(playlist_items)
        .values(item)
        .returning(id)
        .get_result(db_conn)
}

pub fn update_playlist_item_next_id(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
    next_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlist_items::dsl::*;
    diesel::update(playlist_items)
        .filter(id.eq(item_id))
        .set(next.eq(next_id))
        .execute(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::PlaylistItem, item_id.into())
            })
        })
}

pub fn update_playlist_item_prev_id(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
    prev_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlist_items::dsl::*;
    diesel::update(playlist_items)
        .filter(id.eq(item_id))
        .set(prev.eq(prev_id))
        .execute(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::PlaylistItem, item_id.into())
            })
        })
}

pub fn update_playlist_item_prev_and_next_id(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
    prev_id: Option<PlaylistItemId>,
    next_id: Option<PlaylistItemId>,
) -> ResourceQueryResult<()> {
    use crate::schema::playlist_items::dsl::*;
    diesel::update(playlist_items)
        .filter(id.eq(item_id))
        .set((prev.eq(prev_id), next.eq(next_id)))
        .execute(db_conn)
        .map(|_| {})
        .map_err(|e| {
            ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                ResourceQueryError::ResourceNotFound(ResourceType::PlaylistItem, item_id.into())
            })
        })
}

pub fn remove_playlist_item(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
) -> ResourceQueryResult<bool> {
    let item = query_playlist_item(db_conn, item_id)?;
    if let Some(prev) = item.prev {
        update_playlist_item_next_id(db_conn, prev, item.next)?;
    } else {
        update_playlist_first_item(db_conn, item.playlist_id, item.next)?;
    }
    if let Some(next) = item.next {
        update_playlist_item_prev_id(db_conn, next, item.prev)?;
    } else {
        update_playlist_last_item(db_conn, item.playlist_id, item.prev)?;
    }
    let media = query_media_with_id(db_conn, item.media_id)?;
    let playlist = update_playlist(
        db_conn,
        item.playlist_id,
        -media.duration.unwrap_or_default().0,
        -1,
    )?;
    let media_changed = playlist.current_item == Some(item_id);
    if media_changed {
        update_playlist_current_item(db_conn, playlist.id, None)?;
    }

    {
        use crate::schema::playlist_items::dsl::*;
        diesel::delete(playlist_items)
            .filter(id.eq_all(item_id))
            .execute(db_conn)
            .map_err(|e| {
                ResourceQueryError::db_error_if_not_not_found(e).unwrap_or_else(|| {
                    ResourceQueryError::ResourceNotFound(ResourceType::PlaylistItem, item_id.into())
                })
            })?;
    }
    Ok(media_changed)
}
