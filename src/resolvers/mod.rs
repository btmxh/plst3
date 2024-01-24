use anyhow::Result;
use thiserror::Error;
use url::Url;

use crate::db::media::{NewMedia, NewMediaList};

pub mod local;
pub mod youtube;

#[derive(Error, Debug)]
pub enum MediaResolveError {
    #[error("Generic error: {0}")]
    FailedProcessing(#[from] anyhow::Error),
    #[error("Unsupported url")]
    UnsupportedUrl,
    #[error("Invalid resource referenced by url")]
    InvalidResource,
    #[error("Resource referenced by url not found")]
    ResourceNotFound,
}

pub async fn normalize_media_url(url: &str) -> Result<Url, url::ParseError> {
    let url = Url::parse(url)?;
    let url = youtube::normalize_media_url(url);
    let url = local::normalize_media_url(url).await;
    Ok(url)
}

pub async fn resolve_media(url: &Url) -> Result<NewMedia<'static>, MediaResolveError> {
    let mut invalid = vec![];
    let mut not_found = vec![];
    macro_rules! resolve {
        ($resolver: ident) => {
            match $resolver::resolve_media(&url).await {
                Ok(media) => return Ok(media),
                Err(e) => {
                    let resolver = stringify!($resolver);
                    tracing::warn!("error resolving media by {resolver} resolver: {e}");
                    match &e {
                        MediaResolveError::ResourceNotFound => not_found.push(resolver),
                        MediaResolveError::InvalidResource => invalid.push(resolver),
                        _ => return Err(e),
                    };
                }
            };
        };
    }

    resolve!(local);
    resolve!(youtube);

    if invalid.is_empty() {
        Err(MediaResolveError::InvalidResource)
    } else if not_found.is_empty() {
        Err(MediaResolveError::ResourceNotFound)
    } else {
        unreachable!()
    }
}
pub async fn resolve_media_list(
    url: &Url,
) -> Result<(NewMediaList<'static>, Vec<String>), MediaResolveError> {
    let mut invalid = vec![];
    let mut not_found = vec![];
    macro_rules! resolve {
        ($resolver: ident) => {
            match $resolver::resolve_media_list(&url).await {
                Ok(media_list) => return Ok(media_list),
                Err(e) => {
                    let resolver = stringify!($resolver);
                    tracing::warn!("error resolving media list by {resolver} resolver: {e}");
                    match &e {
                        MediaResolveError::ResourceNotFound => not_found.push(resolver),
                        MediaResolveError::InvalidResource => invalid.push(resolver),
                        _ => return Err(e),
                    };
                }
            };
        };
    }

    resolve!(local);
    resolve!(youtube);

    if invalid.is_empty() {
        Err(MediaResolveError::InvalidResource)
    } else if not_found.is_empty() {
        Err(MediaResolveError::ResourceNotFound)
    } else {
        unreachable!()
    }
}
