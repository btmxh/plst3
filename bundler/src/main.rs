use anyhow::Context;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
pub mod bundler;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();
    let _b = bundler::launch_bundler().await?;
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        tx.send(()).ok();
    })
    .context("unable to set Ctrl-C handler")?;
    rx.recv()?;
    Ok(())
}
