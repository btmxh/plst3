use std::{fmt::Display, str::FromStr};

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
use time::PrimitiveDateTime;

use crate::context::app::AppState;

use super::{
    media::MediaId,
    playlist::{update_playlist_current_item, PlaylistId},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, FromSqlRow, AsExpression)]
#[diesel(sql_type = Integer)]
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
) -> Result<Option<PlaylistItem>> {
    use crate::schema::playlist_items::dsl::*;
    let mut matches: Vec<PlaylistItem> = playlist_items
        .filter(id.eq(item_id))
        .limit(1)
        .select(PlaylistItem::as_select())
        .load(db_conn)
        .context("unable to query playlist item from DB")?;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(matches.swap_remove(0)))
    }
}

pub fn insert_playlist_item(
    db_conn: &mut SqliteConnection,
    item: NewPlaylistItem,
) -> Result<PlaylistItemId> {
    use crate::schema::playlist_items::dsl::*;
    diesel::insert_into(playlist_items)
        .values(item)
        .returning(id)
        .get_result(db_conn)
        .context("unable to insert playlist item to DB")
}

pub fn update_playlist_item_next_id(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
    next_id: Option<PlaylistItemId>,
) -> Result<()> {
    use crate::schema::playlist_items::dsl::*;
    diesel::update(playlist_items)
        .filter(id.eq(item_id))
        .set(next.eq(next_id))
        .execute(db_conn)
        .context("unable to update playlist item next id")
        .map(|_| {})
}

pub fn update_playlist_item_prev_id(
    db_conn: &mut SqliteConnection,
    item_id: PlaylistItemId,
    prev_id: Option<PlaylistItemId>,
) -> Result<()> {
    use crate::schema::playlist_items::dsl::*;
    diesel::update(playlist_items)
        .filter(id.eq(item_id))
        .set(prev.eq(prev_id))
        .execute(db_conn)
        .context("unable to update playlist item next id")
        .map(|_| {})
}

