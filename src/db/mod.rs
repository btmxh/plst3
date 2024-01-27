use self::{
    media::{MediaId, MediaListId},
    playlist::PlaylistId,
    playlist_item::PlaylistItemId,
};
use anyhow::{Context, Result};
use diesel::{r2d2::ConnectionManager, SqliteConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use r2d2::Pool;
use std::fmt::Display;
use thiserror::Error;

pub mod media;
pub mod playlist;
pub mod playlist_item;

pub type SqliteConnectionPool = Pool<ConnectionManager<SqliteConnection>>;

pub fn establish_connection() -> Result<SqliteConnectionPool> {
    const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");
    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not specified")?;
    let db_conn = ConnectionManager::<SqliteConnection>::new(db_url);
    let db_pool = Pool::builder()
        .build(db_conn)
        .context("unable to build DB connection pool")?;
    db_pool
        .get()?
        .run_pending_migrations(MIGRATIONS)
        .expect("unable to run pending migrations");
    Ok(db_pool)
}

#[derive(Error, Debug)]
pub enum ResourceQueryError {
    #[error("{}", match .1 {
        Some(id) => format!("{:?} not found with ID {}", .0, id),
        None => format!("{:?} not found", .0)
    })]
    ResourceNotFound(ResourceType, Option<ResourceId>),
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
}
pub type ResourceQueryResult<T> = Result<T, ResourceQueryError>;

impl ResourceQueryError {
    pub fn db_error_if_not_not_found(error: diesel::result::Error) -> Option<Self> {
        match error {
            diesel::result::Error::NotFound => None,
            error => Some(Self::DatabaseError(error)),
        }
    }
}

#[derive(Debug)]
pub enum ResourceType {
    Media,
    Playlist,
    PlaylistItem,
    MediaList,
}

#[derive(Debug)]
pub struct ResourceId(pub i32);

impl From<PlaylistId> for ResourceId {
    fn from(value: PlaylistId) -> Self {
        Self(value.0)
    }
}

impl From<PlaylistId> for Option<ResourceId> {
    fn from(value: PlaylistId) -> Self {
        Some(ResourceId(value.0))
    }
}

impl From<MediaId> for ResourceId {
    fn from(value: MediaId) -> Self {
        Self(value.0)
    }
}

impl From<MediaId> for Option<ResourceId> {
    fn from(value: MediaId) -> Self {
        Some(ResourceId(value.0))
    }
}

impl From<PlaylistItemId> for ResourceId {
    fn from(value: PlaylistItemId) -> Self {
        Self(value.0)
    }
}

impl From<PlaylistItemId> for Option<ResourceId> {
    fn from(value: PlaylistItemId) -> Self {
        Some(ResourceId(value.0))
    }
}

impl From<MediaListId> for ResourceId {
    fn from(value: MediaListId) -> Self {
        Self(value.0)
    }
}

impl From<MediaListId> for Option<ResourceId> {
    fn from(value: MediaListId) -> Self {
        Some(ResourceId(value.0))
    }
}

impl Display for ResourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
