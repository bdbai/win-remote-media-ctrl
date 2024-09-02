use std::io;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use base64::prelude::*;
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

pub struct MediaManager {
    winrt_control: WinrtControl,
}

impl MediaManager {
    pub async fn new() -> io::Result<Self> {
        Ok(Self {
            winrt_control: WinrtControl::create().await?,
        })
    }

    pub async fn get_media_info(&mut self) -> io::Result<Option<MediaInfo>> {
        if self.winrt_control.is_qqmusic_current() {
            if let Ok(Some(qq_info)) = { QQMUSIC_PROCESS.lock().unwrap().collect_full_info() } {
                return Ok(Some(qq_info.into()));
            }
        }
        let winrt_info = self.winrt_control.get_media_info().await?;
        Ok(winrt_info.map(Into::into))
    }

    pub async fn get_timeline_state(&mut self) -> io::Result<Option<TimelineState>> {
        if self.winrt_control.is_qqmusic_current() {
            if let Ok(Some(qq_info)) = { QQMUSIC_PROCESS.lock().unwrap().collect_raw_info() } {
                return Ok(Some(qq_info.into()));
            }
        }
        Ok(self.get_media_info().await?.map(|info| info.timeline))
    }

    pub async fn get_album_image(&mut self) -> io::Result<Option<AlbumImage>> {
        if self.winrt_control.is_qqmusic_current() {
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
        self.winrt_control.get_album_img().await
    }

    pub async fn media_change(&mut self) -> io::Result<()> {
        self.winrt_control.media_change().await
    }
}

async fn get_file_mime(path: &HSTRING) -> io::Result<String> {
    let file = windows::Storage::StorageFile::GetFileFromPathAsync(path)?.await?;
    Ok(file.ContentType()?.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_media_info() -> io::Result<()> {
        let mut manager = MediaManager::new().await?;
        let info = manager.get_media_info().await?;
        println!("{:#?}", info);
        Ok(())
    }

    #[tokio::test]
    async fn test_get_album_image() -> io::Result<()> {
        let mut manager = MediaManager::new().await?;
        let img = manager.get_album_image().await?;
        println!("{:?}", img);
        Ok(())
    }

    #[tokio::test]
    async fn test_media_change() -> io::Result<()> {
        let mut manager = MediaManager::new().await?;
        manager.media_change().await?;
        println!("media changed");
        Ok(())
    }
}
