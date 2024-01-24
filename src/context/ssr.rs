use axum::{response::Html, routing::get};
use sailfish::TemplateOnce;

use super::{app::AppRouter, ResponseResult};

pub fn ssr_router() -> AppRouter {
    AppRouter::new()
        .route("/", get(index))
        .route("/index", get(index))
        .route("/watch", get(watch))
}

#[derive(TemplateOnce)]
#[template(path = "index.stpl")]
struct IndexTemplate;

async fn index() -> ResponseResult<Html<String>> {
    Ok(Html(IndexTemplate.render_once()?))
}

#[derive(TemplateOnce)]
#[template(path = "watch.stpl")]
struct WatchTemplate;

async fn watch() -> ResponseResult<Html<String>> {
    Ok(Html(WatchTemplate.render_once()?))
}
