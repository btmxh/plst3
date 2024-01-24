use anyhow::{Context, Result};
use bundler::launch_bundler;
use context::create_app_router;

use dotenvy::dotenv;

use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod bundler;
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

    let _bundler = launch_bundler().context("unable to launch web bundler");

    let app = create_app_router()
        .await
        .context("unable to create app router")?;
    let listener = TcpListener::bind("0.0.0.0:7272")
        .await
        .context("unable to bind TcpListener")?;
    axum::serve(listener, app)
        .await
        .context("unable to serve axum server")?;

    Ok(())
}
