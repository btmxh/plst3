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
    dotenv().context("unable to load .env")?;
    #[cfg(feature = "journald")]
    {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_journald::layer()?)
            .init();
    }

    #[cfg(not(feature = "journald"))]
    {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer().with_target(false))
            .init();
    }

    let app = create_app_router()
        .await
        .context("unable to create app router")?;
    let listener = TcpListener::bind(
        std::env::var("PLST_ADDR")
            .map(Cow::Owned)
            .unwrap_or_else(|_| "localhost:7272".into())
            .as_ref(),
    )
    .await
    .context("unable to bind TcpListener")?;
    axum::serve(listener, app)
        .await
        .context("unable to serve axum server")?;

    Ok(())
}
