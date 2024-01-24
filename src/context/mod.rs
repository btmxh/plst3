use self::app::AppState;
use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};

pub mod app;
mod playlist;
mod ssr;
mod static_files;
mod ws;

pub async fn create_app_router() -> Result<Router> {
    let app = AppState::new()
        .await
        .context("unable to create app state")?;
    Ok(app.create_router())
}

type ResponseResult<T> = Result<T, ResponseAnyhowError>;
struct ResponseAnyhowError(anyhow::Error);

impl<E> From<E> for ResponseAnyhowError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        Self(value.into())
    }
}

impl IntoResponse for ResponseAnyhowError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}
