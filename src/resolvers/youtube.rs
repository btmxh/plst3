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

fn check_video_id(id: &str) -> bool {
    const VIDEO_ID_LENGTH: usize = 11;
    id.len() == VIDEO_ID_LENGTH
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn check_normalized_youtube_url(url: &Url) -> YoutubeUrlParseResult {
    if url.scheme() != "https" {
        return YoutubeUrlParseResult::Invalid;
    }

    {
        let path = &url.path()[1..];
        if check_video_id(path) && url.host_str() == Some("youtu.be") {
            return YoutubeUrlParseResult::Video(path.into());
        }
    }

    {
        let path = url.path();
        let list_id = url
            .query_pairs()
            .find(|(key, _)| key == "list")
            .map(|(_, value)| value);
        if path == "/playlist" && url.host_str() == Some("youtube.com") {
            if let Some(id) = list_id {
                return YoutubeUrlParseResult::Playlist(id.into_owned().into());
            }
        }
    }

    YoutubeUrlParseResult::Invalid
}

pub fn normalize_media_url(url: Url) -> Url {
    if !url
        .host_str()
        .map(|host| host.contains("youtube") || host.contains("youtu.be") || host.contains("yt.be"))
        .unwrap_or_default()
    {
        return url;
    }

    {
        let video_id = url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value)
            .unwrap_or(url.path().into());
        if check_video_id(&video_id) {
            return Url::parse(&format!("https://youtu.be/{video_id}")).unwrap_or(url);
        }
    }

    {
        let list_id = url
            .query_pairs()
            .find(|(key, _)| key == "list")
            .map(|(_, value)| value);
        if let Some(list_id) = list_id {
            return Url::parse(&format!("https://youtube.com/playlist?list={list_id}"))
                .unwrap_or(url);
        }
    }

    url
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
            media_type: "yt".into(),
        }),
        Ok(_) => Err(MediaResolveError::InvalidMedia),
        Err(youtube_dl::Error::Json(_)) => Err(MediaResolveError::MediaNotFound),
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
                total_duration: playlist
                    .entries
                    .as_ref()
                    .map(|p| {
                        p.iter()
                            .filter_map(|video| video.duration.as_ref())
                            .filter_map(|duration| duration.as_f64())
                            .map(|seconds| seconds.round() as i32)
                            .sum()
                    })
                    .unwrap_or_default(),
            },
            playlist
                .entries
                .unwrap_or_default()
                .into_iter()
                .map(|video| youtube_video_url(&video.id))
                .collect(),
        )),
        Ok(_) => Err(MediaResolveError::InvalidMedia),
        Err(youtube_dl::Error::Json(_)) => Err(MediaResolveError::MediaNotFound),
        Err(e) => Err(MediaResolveError::FailedProcessing(e.into())),
    }
}

pub fn get_media_thumbnail_url(media_url: &str) -> Option<String> {
    let url = Url::parse(media_url).ok()?;
    if let YoutubeUrlParseResult::Video(id) = check_normalized_youtube_url(&url) {
        Some(format!("https://img.youtube.com/vi/{id}/maxresdefault.jpg"))
    } else {
        None
    }
}
