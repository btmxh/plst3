use super::{app::AppRouter, ResponseResult};
use crate::db::playlist::PlaylistId;
use axum::{extract::Path, response::Html, routing::get};
use sailfish::TemplateOnce;

pub fn ssr_router() -> AppRouter {
    AppRouter::new()
        .route("/", get(index))
        .route("/index", get(index))
        .route("/watch/:id", get(watch))
}

#[derive(TemplateOnce)]
#[template(path = "index.stpl")]
struct IndexTemplate;

async fn index() -> ResponseResult<Html<String>> {
    Ok(Html(IndexTemplate.render_once()?))
}

#[derive(TemplateOnce)]
#[template(path = "watch.stpl")]
struct WatchTemplate {
    pid: PlaylistId,
}

async fn watch(Path(pid): Path<i32>) -> ResponseResult<Html<String>> {
    Ok(Html(
        WatchTemplate {
            pid: PlaylistId(pid),
        }
        .render_once()?,
    ))
}
