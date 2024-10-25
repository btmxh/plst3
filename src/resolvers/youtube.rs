use std::borrow::Cow;

use anyhow::Result;
use lazy_static::lazy_static;
use url::Url;
use youtube_dl::{YoutubeDl, YoutubeDlOutput};

use crate::db::media::{NewMedia, NewMediaList};

use super::MediaResolveError;

lazy_static! {
    static ref FORCE_IPV4: bool = std::env::var("YTDL_FORCE_IPV4")
        .ok()
        .and_then(|env| env.parse::<bool>().ok())
        .unwrap_or_default();
}

async fn run_ytdl(url: impl Into<String>) -> Result<YoutubeDlOutput, youtube_dl::Error> {
    let mut builder = YoutubeDl::new(url);
    builder.extra_arg("--ignore-no-formats-error");
    if *FORCE_IPV4 {
        builder.extra_arg("--force-ipv4");
    }
    builder.run_async().await
}

pub fn youtube_video_url_string(id: &str) -> String {
    format!("https://youtu.be/{id}")
}

pub fn youtube_list_url_string(id: &str) -> String {
    format!("https://youtube.com/playlist?list={id}")
}

pub fn youtube_video_url(id: &str) -> Url {
    Url::parse(&youtube_video_url_string(id))
        .expect("invalid id, sanitize with check_video_id first")
}

pub fn youtube_list_url(id: &str) -> Url {
    Url::parse(&youtube_list_url_string(id)).expect("invalid id, sanitize with check_list_id first")
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

fn check_list_id(id: &str) -> bool {
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn check_normalized_youtube_url(url: &Url) -> YoutubeUrlParseResult {
    if url.scheme() != "https" {
        return YoutubeUrlParseResult::Invalid;
    }

    {
        let path = &url.path()[1..];
        if check_video_id(path)
            && (url.host_str() == Some("youtu.be") || url.host_str() == Some("yt.be"))
        {
            return YoutubeUrlParseResult::Video(path.into());
        }
    }

    {
        let video_id = url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value)
            .filter(|id| check_video_id(id));
        if let Some(video_id) = video_id {
            tracing::info!("{url:?}");
            if url.host_str() == Some("youtube.com") || url.host_str() == Some("www.youtube.com") {
                return YoutubeUrlParseResult::Video(video_id);
            }
        }
    }

    {
        let path = url.path();
        let list_id = url
            .query_pairs()
            .find(|(key, _)| key == "list")
            .map(|(_, value)| value);
        if path == "/playlist"
            && (url.host_str() == Some("youtube.com") || url.host_str() == Some("www.youtube.com"))
        {
            if let Some(id) = list_id.filter(|id| check_list_id(id)) {
                return YoutubeUrlParseResult::Playlist(id.into_owned().into());
            }
        }
    }

    YoutubeUrlParseResult::Invalid
}

pub fn normalize_media_url(url: Url) -> Url {
    match check_normalized_youtube_url(&url) {
        YoutubeUrlParseResult::Video(id) => youtube_video_url(&id),
        YoutubeUrlParseResult::Playlist(id) => youtube_list_url(&id),
        YoutubeUrlParseResult::Invalid => url,
    }
}

pub async fn resolve_media(url: &Url) -> Result<NewMedia<'static>, MediaResolveError> {
    if !matches!(
        check_normalized_youtube_url(url),
        YoutubeUrlParseResult::Video(_)
    ) {
        return Err(MediaResolveError::UnsupportedUrl);
    }
    match run_ytdl(url.as_str()).await {
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
    match run_ytdl(url.as_str()).await {
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
                .map(|video| youtube_video_url_string(&video.id))
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
