use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use windows::core::ComInterface;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::media::AlbumImage;

pub(super) struct WinrtControl {
    _is_qqmusic_current: Arc<AtomicBool>,
    manager: GlobalSystemMediaTransportControlsSessionManager,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WinrtMediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub position: i64,
    pub duration: u64,
    pub paused: bool,
}

impl WinrtControl {
    pub(super) async fn create() -> io::Result<Self> {
        Ok(Self {
            manager: GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.await?,
            _is_qqmusic_current: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(super) fn is_qqmusic_current(&self) -> bool {
        let Ok(session) = self.manager.GetCurrentSession() else {
            return false;
        };
        session
            .SourceAppUserModelId()
            .unwrap()
            .to_string_lossy()
            .contains("QQMusic.exe")
    }

    pub(super) async fn _media_change(&self) {
        todo!()
    }

    pub(super) async fn get_media_info(&self) -> io::Result<WinrtMediaInfo> {
        let session = self.manager.GetCurrentSession()?;

        let media_property = session.TryGetMediaPropertiesAsync()?.await?;
        let title = media_property.Title()?.to_string_lossy();
        let artist = media_property.Artist()?.to_string_lossy();
        let album = media_property.AlbumTitle()?.to_string_lossy();

        let timeline_property = session.GetTimelineProperties()?;
        let position = timeline_property.Position()?.Duration as _;
        let duration = timeline_property.EndTime()?.Duration as _;

        let paused = session.GetPlaybackInfo()?.PlaybackStatus()?.0 != 4;

        Ok(WinrtMediaInfo {
            title,
            artist,
            album,
            position,
            duration,
            paused,
        })
    }

    pub(super) async fn get_album_img(&self) -> io::Result<AlbumImage> {
        use base64::prelude::*;

        let session = self.manager.GetCurrentSession()?;
        let media_property = session.TryGetMediaPropertiesAsync()?.await?;
        let mut content_type;
        let base64 = {
            let reader = {
                let read_task = media_property.Thumbnail()?.OpenReadAsync()?;
                let stream = read_task.await?;
                content_type = stream.ContentType()?.to_string_lossy();
                DataReader::CreateDataReader(&stream)?
            };
            reader.LoadAsync(1024 * 512)?.await?;
            let buf = reader.DetachBuffer()?;
            let size = buf.Length()? as usize;
            unsafe {
                let iba = buf.cast::<IBufferByteAccess>()?.Buffer()?;
                let buf = std::slice::from_raw_parts(iba, size);
                BASE64_STANDARD.encode(buf)
            }
        };
        if let Some(comma_pos) = content_type.find(',') {
            content_type.replace_range(comma_pos.., "");
        }
        Ok(AlbumImage::Blob {
            mime: content_type,
            base64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_winrt_control() -> io::Result<()> {
        let control = WinrtControl::create().await?;
        let info = control.get_media_info().await?;
        println!("{:#?}", info);
        let img = control.get_album_img().await?;
        println!("{:?}", img);
        Ok(())
    }
}
