use super::{
    media::{DurationWrapper, Media, MediaId},
    playlist_item::{
        insert_playlist_item, query_playlist_item, update_playlist_item_next_id,
        update_playlist_item_prev_id, NewPlaylistItem, PlaylistItem, PlaylistItemId,
    },
};
use anyhow::{anyhow, Context, Result};
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
use std::{fmt::Display, ops::Add, str::FromStr};
use time::{Duration, PrimitiveDateTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, FromSqlRow, AsExpression)]
#[diesel(sql_type = Integer)]
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
) -> Result<Option<Playlist>> {
    use crate::schema::playlists::dsl::*;
    let mut matches: Vec<Playlist> = playlists
        .filter(id.eq(playlist_id))
        .limit(1)
        .select(Playlist::as_select())
        .load(db_conn)
        .context("error querying playlist")?;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(matches.swap_remove(0)))
    }
}

fn append_to_playlist_single(
    db_conn: &mut SqliteConnection,
    new_playlist_item: NewPlaylistItem,
) -> Result<PlaylistItemId> {
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
    mut prev: Option<PlaylistItemId>,
    media_ids: &[MediaId],
    total_duration: Duration,
) -> Result<bool> {
    if media_ids.is_empty() {
        return Ok(false);
    }

    let next = match prev {
        Some(id) => {
            query_playlist_item(db_conn, id)?
                .ok_or_else(|| anyhow!("Playlist item not found"))?
                .next
        }
        None => {
            query_playlist_from_id(db_conn, playlist_id)?
                .ok_or_else(|| anyhow!("Playlist not found"))?
                .first_playlist_item
        }
    };
    for media_id in media_ids.iter().cloned() {
        prev = Some(append_to_playlist_single(
            db_conn,
            NewPlaylistItem {
                playlist_id,
                media_id,
                prev,
                next,
            },
        )?);
    }
    update_playlist(db_conn, playlist_id, total_duration, media_ids.len() as i32)?;
    Ok(true)
}

pub fn update_playlist_first_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> Result<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(first_playlist_item.eq(item_id))
        .execute(db_conn)
        .context("unable to update playlist first item")
        .map(|_| {})
}

pub fn update_playlist_last_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> Result<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(last_playlist_item.eq(item_id))
        .execute(db_conn)
        .context("unable to update playlist first item")
        .map(|_| {})
}

pub(crate) fn update_playlist_current_item(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    item_id: Option<PlaylistItemId>,
) -> Result<()> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set(current_item.eq(item_id))
        .execute(db_conn)
        .context("unable to update playlist first item")
        .map(|_| {})
}

pub async fn create_empty_playlist(
    db_conn: &mut SqliteConnection,
    playlist_title: &str,
) -> Result<PlaylistId> {
    use crate::schema::playlists::dsl::*;
    diesel::insert_into(playlists)
        .values(title.eq(playlist_title))
        .returning(id)
        .get_result(db_conn)
        .context("unable to create new playlist")
        .map(PlaylistId)
}

pub fn update_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    add_duration: Duration,
    num_add_items: i32,
) -> Result<Playlist> {
    use crate::schema::playlists::dsl::*;
    diesel::update(playlists)
        .filter(id.eq(playlist_id))
        .set((
            total_duration.eq(total_duration + add_duration.as_seconds_f64().round() as i32),
            num_items.eq(num_items + num_add_items),
        ))
        .get_result(db_conn)
        .context("unable to increase playlist duration")
}
