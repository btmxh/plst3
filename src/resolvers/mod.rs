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
    InvalidMedia,
    #[error("Resource referenced by url not found")]
    MediaNotFound,
    #[error("Invalid media type")]
    InvalidType,
}

pub async fn normalize_media_url(url: &str) -> Result<Url, url::ParseError> {
    let url = Url::parse(url)?;
    let url = youtube::normalize_media_url(url);
    let url = local::normalize_media_url(url).await;
    Ok(url)
}

pub async fn resolve_media(
    url: &Url,
    media_type: Option<&str>,
) -> Result<NewMedia<'static>, MediaResolveError> {
    let mut invalid = vec![];
    let mut not_found = vec![];
    macro_rules! resolve {
        ($resolver: ident, $typename: expr) => {
            if media_type.map(|t| t == $typename).unwrap_or(true) {
                match $resolver::resolve_media(&url).await {
                    Ok(media) => return Ok(media),
                    Err(e) => {
                        let resolver = stringify!($resolver);
                        tracing::warn!("error resolving media by {resolver} resolver: {e}");
                        match &e {
                            MediaResolveError::MediaNotFound => not_found.push(resolver),
                            MediaResolveError::InvalidMedia => invalid.push(resolver),
                            _ => return Err(e),
                        };
                    }
                };
            }
        };
    }

    resolve!(local, "local");
    resolve!(youtube, "yt");

    if invalid.is_empty() {
        Err(MediaResolveError::InvalidMedia)
    } else if not_found.is_empty() {
        Err(MediaResolveError::MediaNotFound)
    } else {
        Err(MediaResolveError::InvalidType)
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
                        MediaResolveError::MediaNotFound => not_found.push(resolver),
                        MediaResolveError::InvalidMedia => invalid.push(resolver),
                        _ => return Err(e),
                    };
                }
            };
        };
    }

    resolve!(local);
    resolve!(youtube);

    if invalid.is_empty() {
        Err(MediaResolveError::InvalidMedia)
    } else if not_found.is_empty() {
        Err(MediaResolveError::MediaNotFound)
    } else {
        unreachable!()
    }
}

pub fn get_media_thumbnail_url(media_type: &str, media_url: &str) -> Option<String> {
    if media_type == "yt" {
        return youtube::get_media_thumbnail_url(media_url);
    }

    None
}
