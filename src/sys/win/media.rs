use std::io;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use base64::prelude::*;
use tokio::sync::OnceCell;
use windows::core::HSTRING;

use super::qqmusic::{FullInfo as QQMusicFullInfo, QQMusicProcess, RawInfo as QQMusicRawInfo};
use super::winrt_control::{WinrtControl, WinrtMediaInfo};
use crate::media::{AlbumImage, MediaInfo, TimelineState, TrackInfo};

static QQMUSIC_PROCESS: LazyLock<Mutex<QQMusicProcess>> =
    LazyLock::new(|| Mutex::new(QQMusicProcess::new()));

impl From<QQMusicFullInfo> for MediaInfo {
    fn from(qq_info: QQMusicFullInfo) -> Self {
        MediaInfo {
            track: TrackInfo {
                title: qq_info.title,
                artist: qq_info.artist,
                album: qq_info.album,
            },
            timeline: TimelineState {
                duration: Duration::from_millis(qq_info.duration as _),
                position: Duration::from_millis(qq_info.position as _),
                paused: qq_info.paused || qq_info.duration == 0,
            },
        }
    }
}

impl From<QQMusicRawInfo> for TimelineState {
    fn from(qq_info: QQMusicRawInfo) -> Self {
        TimelineState {
            duration: Duration::from_millis(qq_info.track_info.duration as _),
            position: Duration::from_millis(qq_info.track_info.position as _),
            paused: qq_info.paused != 0 || qq_info.track_info.duration == 0,
        }
    }
}

impl From<WinrtMediaInfo> for MediaInfo {
    fn from(winrt_info: WinrtMediaInfo) -> Self {
        MediaInfo {
            track: TrackInfo {
                title: winrt_info.title,
                artist: winrt_info.artist,
                album: winrt_info.album,
            },
            timeline: TimelineState {
                duration: Duration::from_nanos((winrt_info.duration * 100) as _),
                position: Duration::from_nanos((winrt_info.position * 100) as _),
                paused: winrt_info.paused,
            },
        }
    }
}

async fn get_global_winrt_control() -> &'static WinrtControl {
    static WINRT_CONTROL: OnceCell<WinrtControl> = OnceCell::const_new();
    WINRT_CONTROL
        .get_or_init(|| async {
            WinrtControl::create()
                .await
                .expect("Initializing WinRT media control manager")
        })
        .await
}

pub async fn get_media_info() -> io::Result<Option<MediaInfo>> {
    let winrt_control = get_global_winrt_control().await;
    if winrt_control.is_qqmusic_current() {
        if let Ok(Some(qq_info)) = { QQMUSIC_PROCESS.lock().unwrap().collect_full_info() } {
            return Ok(Some(qq_info.into()));
        }
    }
    let winrt_info = winrt_control.get_media_info().await?;
    Ok(winrt_info.map(Into::into))
}

pub async fn get_timeline_state() -> io::Result<Option<TimelineState>> {
    let winrt_control = get_global_winrt_control().await;
    if winrt_control.is_qqmusic_current() {
        if let Ok(Some(qq_info)) = { QQMUSIC_PROCESS.lock().unwrap().collect_raw_info() } {
            return Ok(Some(qq_info.into()));
        }
    }
    Ok(get_media_info().await?.map(|info| info.timeline))
}

async fn get_file_mime(path: &HSTRING) -> io::Result<String> {
    let file = windows::Storage::StorageFile::GetFileFromPathAsync(path)?.await?;
    Ok(file.ContentType()?.to_string_lossy())
}

pub async fn get_album_image() -> io::Result<Option<AlbumImage>> {
    let winrt_control = get_global_winrt_control().await;
    if winrt_control.is_qqmusic_current() {
        if let Ok(Some(qq_info)) = {
            let mut guard = QQMUSIC_PROCESS.lock().unwrap();
            guard.collect_full_info()
        } {
            let maybe_url = qq_info.album_img_path_or_url.to_string_lossy();
            if maybe_url.starts_with("http") {
                return Ok(Some(AlbumImage::Url(maybe_url.into_owned())));
            }
            let path_hstr = HSTRING::from(&qq_info.album_img_path_or_url);
            let (mime, img_buf) = tokio::join!(
                get_file_mime(&path_hstr),
                tokio::fs::read(qq_info.album_img_path_or_url),
            );
            return Ok(Some(AlbumImage::Blob {
                mime: mime?,
                base64: BASE64_STANDARD.encode(&img_buf?),
            }));
        }
    }
    winrt_control.get_album_img().await
}

pub async fn media_changed() -> io::Result<()> {
    let winrt_control = get_global_winrt_control().await;
    winrt_control.media_changed().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_media_info() -> io::Result<()> {
        let info = get_media_info().await?;
        println!("{:#?}", info);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_album_image() -> io::Result<()> {
        let img = get_album_image().await?;
        println!("{:?}", img);
        Ok(())
    }
}
