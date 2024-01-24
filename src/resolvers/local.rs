use super::MediaResolveError;
use crate::db::media::{NewMedia, NewMediaList};
use anyhow::{anyhow, Context, Result};
use std::{borrow::Cow, ffi::OsStr, io::ErrorKind, path::Path, sync::Once};
use tokio::{fs::canonicalize, process::Command};
use url::Url;

async fn url_from_file_path(path: impl AsRef<Path>) -> Result<String> {
    Ok(Url::from_file_path(
        canonicalize(path)
            .await
            .context("unable to canonicalize path")?,
    )
    .map_err(|_| anyhow!("unable to construct url from file path"))?
    .to_string())
}

async fn url_from_dir_path(path: impl AsRef<Path>) -> Result<String> {
    Ok(Url::from_directory_path(
        canonicalize(path)
            .await
            .context("unable to canonicalize path")?,
    )
    .map_err(|_| anyhow!("unable to construct url from directory path"))?
    .to_string())
}

async fn get_media_duration(path: &Path) -> Result<Option<i32>> {
    static FFPROBE_ENV: Once = Once::new();
    let executable: Cow<'static, OsStr> = std::env::var_os("FFPROBE_EXECUTABLE")
        .map(Cow::Owned)
        .unwrap_or_else(|| {
            FFPROBE_ENV.call_once(|| {
                tracing::info!(
                    "FFPROBE_EXECUTABLE environment variable not provided, defaulting to 'ffprobe'"
                )
            });
            OsStr::new("ffprobe").into()
        });
    let output = Command::new(&executable)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .await
        .context("unable to execute ffprobe process")?;
    if output.status.success() {
        tracing::info!("ffprobe succeeded");
        return Ok(std::str::from_utf8(&output.stdout)
            .context("unable to convert duration to utf8")
            .and_then(|s| Ok(s.trim().parse::<f64>()?))
            .map_err(|e| tracing::warn!("error interpreting duration returned from ffprobe: {e}"))
            .map(|secs| secs.round() as i32)
            .ok());
    }
    todo!()
}

pub async fn normalize_media_url(url: Url) -> Url {
    if url.scheme() == "file" {
        if let Ok(path) = url.to_file_path() {
            if let Ok(path) = tokio::fs::canonicalize(path).await {
                if let Ok(metadata) = tokio::fs::metadata(&path).await {
                    if metadata.is_file() {
                        return Url::from_file_path(path).unwrap_or(url);
                    } else {
                        return Url::from_directory_path(path).unwrap_or(url);
                    }
                }
            }
        }
    }

    url
}

pub async fn resolve_media(url: &Url) -> Result<NewMedia<'static>, MediaResolveError> {
    if url.scheme() == "file" {
        if let Ok(path) = url.to_file_path() {
            return match tokio::fs::metadata(&path).await {
                Ok(metadata) if metadata.is_file() => {
                    let title: Cow<'static, str> = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned().into())
                        .unwrap_or_else(|| "<invalid basename>".into());
                    Ok(NewMedia {
                        title,
                        artist: "<local file>".into(),
                        duration: get_media_duration(&path).await?,
                        url: url_from_file_path(path)
                            .await
                            .context("unable to create url for file path")?
                            .into(),
                        media_type: "local".into(),
                    })
                }
                Ok(_) => Err(MediaResolveError::InvalidResource),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    Err(MediaResolveError::ResourceNotFound)
                }
                Err(e) => Err(MediaResolveError::FailedProcessing(e.into())),
            };
        }
    }

    Err(MediaResolveError::InvalidResource)
}

pub async fn resolve_media_list(
    url: &Url,
) -> Result<(NewMediaList<'static>, Vec<String>), MediaResolveError> {
    if url.scheme() == "file" {
        if let Ok(path) = url.to_file_path() {
            return match tokio::fs::metadata(&path).await {
                Ok(metadata) if metadata.is_dir() => {
                    let title: Cow<'static, str> = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned().into())
                        .unwrap_or_else(|| "<invalid basename>".into());
                    return Ok((
                        NewMediaList {
                            title,
                            artist: "<local directory>".into(),
                            url: url_from_dir_path(path)
                                .await
                                .context("unable to create url for directory")?
                                .into(),
                            media_ids: "".into(),
                        },
                        vec![],
                    ));
                }
                Ok(_) => Err(MediaResolveError::InvalidResource),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    Err(MediaResolveError::ResourceNotFound)
                }
                Err(e) => Err(MediaResolveError::FailedProcessing(e.into())),
            };
        }
    }

    Err(MediaResolveError::InvalidResource)
}
