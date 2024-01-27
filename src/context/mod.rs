use self::app::{AppState, FetchMediaError};
use crate::{
    db::{ResourceId, ResourceQueryError, ResourceType},
    resolvers::MediaResolveError,
};
use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use std::{borrow::Cow, convert::Infallible};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum ResponseError {
    #[error("Generic error: {0}")]
    Generic(#[from] anyhow::Error),
    #[error("{}", match .1 {
        Some(id) => format!("{:?} not found with ID {}", .0, id),
        None => format!("{:?} not found", .0)
    })]
    ResourceNotFound(ResourceType, Option<ResourceId>),
    #[error("Database error: {0}")]
    DatabaseError(#[from] diesel::result::Error),
    #[error("Could not connect to database: {0}")]
    DatabaseConnectionError(#[from] r2d2::Error),
    #[error("Site rendering error: {0}")]
    RenderingError(#[from] sailfish::RenderError),
    #[error("Bad request: {0}")]
    InvalidRequest(Cow<'static, str>),
    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(Cow<'static, str>),
}

impl From<Infallible> for ResponseError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl From<FetchMediaError> for ResponseError {
    fn from(value: FetchMediaError) -> Self {
        match value {
            FetchMediaError::DatabaseError(e) => Self::DatabaseError(e),
            FetchMediaError::ResolveError(e) => match e {
                MediaResolveError::UnsupportedUrl => {
                    Self::UnprocessableEntity("Unsupported URL".into())
                }
                MediaResolveError::FailedProcessing(e) => Self::Generic(e),
                MediaResolveError::InvalidMedia => {
                    Self::UnprocessableEntity("Invalid referenced media".into())
                }
                MediaResolveError::MediaNotFound => {
                    Self::ResourceNotFound(ResourceType::Media, None)
                }
            },
            FetchMediaError::InvalidUrl(e) => {
                Self::InvalidRequest(format!("Invalid URL: {e}").into())
            }
        }
    }
}

impl From<ResourceQueryError> for ResponseError {
    fn from(value: ResourceQueryError) -> Self {
        match value {
            ResourceQueryError::ResourceNotFound(resource, id) => {
                Self::ResourceNotFound(resource, id)
            }
            ResourceQueryError::DatabaseError(error) => Self::DatabaseError(error),
        }
    }
}

impl IntoResponse for ResponseError {
    fn into_response(self) -> Response {
        let code = match &self {
            ResponseError::ResourceNotFound(_, _) => StatusCode::NOT_FOUND,
            ResponseError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, format!("{}", self)).into_response()
    }
}

pub type ResponseResult<T> = Result<T, ResponseError>;
