use std::{borrow::Cow, fmt::Display, ops::Deref, str::FromStr};

use crate::schema::{media_lists, medias};
use anyhow::{Context, Result};
use diesel::{
    deserialize::{FromSql, FromSqlRow},
    expression::AsExpression,
    prelude::*,
    serialize::{IsNull, ToSql},
    sql_types::{Integer, Text},
    sqlite::Sqlite,
};
use sailfish::runtime::Render;
use serde::Serialize;
use time::{Duration, PrimitiveDateTime};
use url::Url;

#[derive(PartialEq, Eq, Clone, Copy, Debug, AsExpression, FromSqlRow, Serialize)]
#[diesel(sql_type = Integer)]
pub struct MediaId(pub i32);

impl FromSql<Integer, Sqlite> for MediaId {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(MediaId(<i32 as FromSql<Integer, Sqlite>>::from_sql(bytes)?))
    }
}

impl ToSql<Integer, Sqlite> for MediaId {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        <i32 as ToSql<Integer, Sqlite>>::to_sql(&self.0, out)
    }
}

impl Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Render for MediaId {
    fn render(&self, b: &mut sailfish::runtime::Buffer) -> Result<(), sailfish::RenderError> {
        self.0.render(b)
    }
}

impl FromStr for MediaId {
    type Err = <i32 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i32>().map(Self)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, AsExpression, FromSqlRow)]
#[diesel(sql_type = Integer)]
pub struct MediaListId(pub i32);

impl FromSql<Integer, Sqlite> for MediaListId {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(MediaListId(<i32 as FromSql<Integer, Sqlite>>::from_sql(
            bytes,
        )?))
    }
}

impl ToSql<Integer, Sqlite> for MediaListId {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        <i32 as ToSql<Integer, Sqlite>>::to_sql(&self.0, out)
    }
}

impl Display for MediaListId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Render for MediaListId {
    fn render(&self, b: &mut sailfish::runtime::Buffer) -> Result<(), sailfish::RenderError> {
        self.0.render(b)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug, FromSqlRow, Serialize)]
pub struct DurationWrapper(pub Duration);

impl AsExpression<Integer> for DurationWrapper {
    type Expression = <i32 as AsExpression<Integer>>::Expression;

    fn as_expression(self) -> Self::Expression {
        <i32 as AsExpression<Integer>>::as_expression(
            i32::try_from(self.0.whole_seconds()).expect("overflow"),
        )
    }
}

impl FromSql<Integer, Sqlite> for DurationWrapper {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        Ok(DurationWrapper(Duration::new(i64::from_sql(bytes)?, 0)))
    }
}

impl ToSql<Integer, Sqlite> for DurationWrapper {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
    ) -> diesel::serialize::Result {
        let value = i32::try_from(self.0.whole_seconds())?;
        out.set_value(value);
        Ok(IsNull::No)
    }
}

impl Deref for DurationWrapper {
    type Target = Duration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(PartialEq, Eq, Debug, Clone, FromSqlRow)]
pub struct MediaIdList(pub Vec<MediaId>);

impl AsExpression<Text> for MediaIdList {
    type Expression = <String as AsExpression<Text>>::Expression;

    fn as_expression(self) -> Self::Expression {
        let text = self
            .0
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
        <String as AsExpression<Text>>::as_expression(text)
    }
}

impl FromSql<Text, Sqlite> for MediaIdList {
    fn from_sql(
        bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        let list = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?
            .split(',')
            .map(|id| id.parse::<MediaId>())
            .collect::<Result<Vec<MediaId>, _>>()?;
        Ok(MediaIdList(list))
    }
}

pub enum MediaIds {
    Single {
        id: MediaId,
    },
    Multiple {
        id: MediaListId,
        medias: MediaIdList,
    },
}
impl MediaIds {
    pub fn new_single(id: MediaId) -> Self {
        Self::Single { id }
    }

    pub fn new_multiple(id: MediaListId, medias: MediaIdList) -> Self {
        Self::Multiple { id, medias }
    }

    pub fn media_ids(self) -> Vec<MediaId> {
        match self {
            Self::Single { id } => vec![id],
            Self::Multiple { medias, .. } => medias.0,
        }
    }
}

#[derive(Queryable, Selectable, Debug, Serialize)]
#[diesel(table_name = crate::schema::medias)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Media {
    pub id: MediaId,
    pub title: String,
    pub artist: String,
    pub duration: Option<DurationWrapper>,
    pub url: String,
    pub media_type: String,
}

#[derive(Queryable, Selectable, Debug)]
#[diesel(table_name = crate::schema::media_lists)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct MediaList {
    pub id: MediaListId,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub media_ids: MediaIdList,
    pub url: String,
    pub add_timestamp: PrimitiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = medias)]
pub struct NewMedia<'a> {
    pub title: Cow<'a, str>,
    pub artist: Cow<'a, str>,
    pub duration: Option<i32>,
    pub url: Cow<'a, str>,
    pub media_type: String,
}

#[derive(Insertable)]
#[diesel(table_name = media_lists)]
pub struct NewMediaList<'a> {
    pub title: Cow<'a, str>,
    pub artist: Cow<'a, str>,
    pub media_ids: Cow<'a, str>,
    pub url: Cow<'a, str>,
}

pub fn query_media_with_id(
    db_conn: &mut SqliteConnection,
    media_id: MediaId,
) -> Result<Option<Media>> {
    use crate::schema::medias::dsl::*;
    let mut matches: Vec<Media> = medias
        .filter(id.eq(media_id))
        .limit(1)
        .select(Media::as_select())
        .load(db_conn)
        .context("error fetching media")?;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(matches.swap_remove(0)))
    }
}

pub fn query_media_with_url(
    db_conn: &mut SqliteConnection,
    media_url: &Url,
) -> Result<Option<Media>> {
    use crate::schema::medias::dsl::*;
    let mut matches: Vec<Media> = medias
        .filter(url.eq(media_url.to_string()))
        .limit(1)
        .select(Media::as_select())
        .load(db_conn)
        .context("error fetching media")?;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(matches.swap_remove(0)))
    }
}

pub fn query_media_list_with_url(
    db_conn: &mut SqliteConnection,
    media_list_url: &Url,
) -> Result<Option<MediaList>> {
    use crate::schema::media_lists::dsl::*;
    let mut matches: Vec<MediaList> = media_lists
        .filter(url.eq(media_list_url.to_string()))
        .limit(1)
        .select(MediaList::as_select())
        .load(db_conn)
        .context("error fetching media list")?;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(matches.swap_remove(0)))
    }
}

pub fn insert_media(db_conn: &mut SqliteConnection, media: NewMedia) -> Result<MediaId> {
    use crate::schema::medias::dsl::*;
    Ok(MediaId(
        diesel::insert_into(medias)
            .values(media)
            .returning(id)
            .get_result(db_conn)
            .context("unable to insert medias to DB")?,
    ))
}

pub fn insert_media_list(
    db_conn: &mut SqliteConnection,
    media_list: NewMediaList,
) -> Result<MediaListId> {
    use crate::schema::media_lists::dsl::*;
    Ok(MediaListId(
        diesel::insert_into(media_lists)
            .values(media_list)
            .returning(id)
            .get_result(db_conn)
            .context("unable to insert media list to DB")?,
    ))
}
