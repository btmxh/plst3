use std::{fmt::Display, str::FromStr};

use anyhow::{Context, Result};
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
use time::PrimitiveDateTime;

use super::{
    media::MediaId,
    playlist_item::{
        insert_playlist_item, query_playlist_item, update_playlist_item_next_id, NewPlaylistItem,
        PlaylistItemId, update_playlist_item_prev_id,
    },
};

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

pub fn append_to_playlist(
    db_conn: &mut SqliteConnection,
    playlist_id: PlaylistId,
    after_id: Option<PlaylistItemId>,
    media_ids: Vec<MediaId>,
) -> Result<bool> {
    if media_ids.is_empty() {
        return Ok(false);
    }

    let prev_item = after_id
        .map(|id| query_playlist_item(db_conn, id))
        .transpose()
        .map(Option::flatten)
        .context("unable to query pivot playlist item")?;
    let next = prev_item.and_then(|p| p.next);
    let last_id = None;
    for media_id in media_ids {
        let prev = last_id.or(after_id);
        let item_id = insert_playlist_item(
            db_conn,
            NewPlaylistItem {
                playlist_id,
                media_id,
                prev,
                next,
            },
        )
        .context("unable to insert playlist item to DB")?;
        if let Some(prev) = prev {
            update_playlist_item_next_id(db_conn, prev, Some(item_id))
                .context("unable to update next playlist item id")?;
        } else {
            update_playlist_first_item(db_conn, playlist_id, Some(item_id))
                .context("unable to update first playlist item id")?;
        }
        if let Some(next) = next {
            update_playlist_item_prev_id(db_conn, next, Some(item_id))
                .context("unable to update prev playlist item id")?;
        } else {
            update_playlist_last_item(db_conn, playlist_id, Some(item_id))
                .context("unable to update last playlist item id")?;
        }
    }
    Ok(true)
}

fn update_playlist_first_item(
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

fn update_playlist_last_item(
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
