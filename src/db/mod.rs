use anyhow::{Context, Result};
use diesel::{r2d2::ConnectionManager, SqliteConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use r2d2::Pool;

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
#[cfg(falseadad)]
fn test() -> Result<()> {
    dotenv().ok();

    let db_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    let mut db_conn = SqliteConnection::establish(&db_url)
        .context("unable to establish connection to sqlite database")?;

    use crate::schema::medias::dsl::*;
    diesel::insert_into(crate::schema::medias::table)
        .values(&NewMedia {
            title: "hello",
            artist: "me",
            duration: None,
            metadata: "{}",
            url: "https://youtu.be/sjfdsfkjds",
        })
        .execute(&mut db_conn)
        .context("expect returning new media")?;

    let results = medias
        .filter(title.eq("hello"))
        .select(Media::as_select())
        .load(&mut db_conn)
        .context("error loading medias")?;
    for media in results {
        println!("{media:?}");
    }
    println!("Hello, world!");
    Ok(())
}
