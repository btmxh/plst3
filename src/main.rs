use std::borrow::Cow;

use anyhow::{Context, Result};
use context::create_app_router;

use dotenvy::dotenv;

use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod context;
pub mod db;
pub mod resolvers;
pub mod schema;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();
    dotenv().context("unable to load .env")?;

    let app = create_app_router()
        .await
        .context("unable to create app router")?;
    let addr = std::env::var("PLST_ADDR")
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed("localhost:7272"));
    let listener = TcpListener::bind(addr.as_ref())
        .await
        .context("unable to bind TcpListener")?;
    axum::serve(listener, app)
        .await
        .context("unable to serve axum server")?;
    Ok(())
}
