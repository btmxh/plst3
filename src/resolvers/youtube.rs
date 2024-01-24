use std::borrow::Cow;

use anyhow::Result;
use url::Url;
use youtube_dl::{YoutubeDl, YoutubeDlOutput};

use crate::db::media::{NewMedia, NewMediaList};

use super::MediaResolveError;

pub fn youtube_video_url(id: &str) -> String {
    format!("https://youtu.be/{id}")
}

pub fn youtube_list_url(id: &str) -> String {
    format!("https://youtube.com/playlist?list={id}")
}

pub enum YoutubeUrlParseResult<'a> {
    Video(Cow<'a, str>),
    Playlist(Cow<'a, str>),
    Invalid,
}

pub fn check_normalized_youtube_url(url: &Url) -> YoutubeUrlParseResult {
    if url.scheme() != "https" {
        return YoutubeUrlParseResult::Invalid;
    }

    {
        const VIDEO_ID_LENGTH: usize = 11;
        let path = &url.path()[1..];
        if path.len() == VIDEO_ID_LENGTH
            && url.host_str() == Some("youtu.be")
            && path.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return YoutubeUrlParseResult::Video(path.into());
        }
    }

    {
        let path = url.path();
        let list_id = url
            .query_pairs()
            .find(|(key, _)| key == "list")
            .map(|(_, value)| value);
        if path == "playlist" && url.host_str() == Some("youtube.com") {
            if let Some(id) = list_id {
                return YoutubeUrlParseResult::Playlist(id.into_owned().into());
            }
        }
    }

    YoutubeUrlParseResult::Invalid
}

pub async fn resolve_media(url: &Url) -> Result<NewMedia<'static>, MediaResolveError> {
    if !matches!(
        check_normalized_youtube_url(url),
        YoutubeUrlParseResult::Video(_)
    ) {
        return Err(MediaResolveError::UnsupportedUrl);
    }
    match YoutubeDl::new(url.to_string())
        .extra_arg("--ignore-no-formats-error")
        .run_async()
        .await
    {
        Ok(YoutubeDlOutput::SingleVideo(video)) => Ok(NewMedia {
            title: video
                .title
                .map(Cow::Owned)
                .unwrap_or("<empty youtube title>".into()),
            artist: video
                .artist
                .or(video.channel)
                .or(video.uploader)
                .map(Cow::Owned)
                .unwrap_or("<empty youtube channel>".into()),
            duration: video
                .duration
                .and_then(|v| v.as_f64())
                .map(|v| v.round() as i32),
            url: url.to_string().into(),
        }),
        Ok(_) => Err(MediaResolveError::InvalidResource),
        Err(youtube_dl::Error::Json(_)) => Err(MediaResolveError::ResourceNotFound),
        Err(e) => Err(MediaResolveError::FailedProcessing(e.into())),
    }
}

pub async fn resolve_media_list(
    url: &Url,
) -> Result<(NewMediaList<'static>, Vec<String>), MediaResolveError> {
    if !matches!(
        check_normalized_youtube_url(url),
        YoutubeUrlParseResult::Playlist(_)
    ) {
        return Err(MediaResolveError::UnsupportedUrl);
    }
    match YoutubeDl::new(url.to_string()).run_async().await {
        Ok(YoutubeDlOutput::Playlist(playlist)) => Ok((
            NewMediaList {
                title: playlist
                    .title
                    .map(Cow::Owned)
                    .unwrap_or("<empty youtube title>".into()),
                artist: playlist
                    .uploader
                    .map(Cow::Owned)
                    .unwrap_or("<empty youtube channel>".into()),
                url: url.to_string().into(),
                media_ids: "".into(),
            },
            playlist
                .entries
                .unwrap_or_default()
                .into_iter()
                .map(|video| youtube_video_url(&video.id))
                .collect(),
        )),
        Ok(_) => Err(MediaResolveError::InvalidResource),
        Err(youtube_dl::Error::Json(_)) => Err(MediaResolveError::ResourceNotFound),
        Err(e) => Err(MediaResolveError::FailedProcessing(e.into())),
    }
}
