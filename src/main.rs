use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
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
    let addr = SocketAddr::from_str(
        std::env::var("PLST_ADDR")
            .as_deref()
            .unwrap_or("localhost:7272"),
    )
    .context("unable to parse address")?;
    axum_server::bind_rustls(
        addr,
        RustlsConfig::from_pem_file(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tls")
                .join("plst3.crt"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tls")
                .join("plst3.key"),
        )
        .await
        .context("unable to load TLS config")?,
    )
    .serve(app.into_make_service())
    .await
    .context("unable to run server")?;

    Ok(())
}
