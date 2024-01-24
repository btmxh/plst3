use anyhow::{Context, Result};
use axum::async_trait;
use mpris_server::{zbus::fdo, Player, RootInterface};

pub struct MprisPlayer {
}


impl MprisPlayer {
    pub async fn new() -> Result<Self> {
        let player = Player::builder("io.github.btmxh.plst3")
            .can_play(true)
            .can_pause(true)
            .can_quit(false)
            .can_seek(false)
            .can_raise(false)
            .can_go_next(true)
            .can_go_previous(true)
            .can_control(true)
            .build()
            .await
            .context("unable to create MPRIS player")?;
        Ok(Self { player })
    }
}
