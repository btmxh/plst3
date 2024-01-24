use tower_http::services::ServeDir;

use super::app::AppRouter;

pub fn static_file_router() -> AppRouter {
    AppRouter::new()
        .nest_service("/assets", ServeDir::new("dist/assets"))
        .nest_service("/styles", ServeDir::new("dist/styles"))
        .nest_service("/scripts", ServeDir::new("dist/scripts"))
}
