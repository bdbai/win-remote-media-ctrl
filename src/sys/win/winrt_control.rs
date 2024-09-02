use std::future::poll_fn;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::task::AtomicWaker;
use windows::core::ComInterface;
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::media::AlbumImage;

pub(super) struct WinrtControl {
    is_qqmusic_current: bool,
    manager: GlobalSystemMediaTransportControlsSessionManager,
    current_session: Option<(
        GlobalSystemMediaTransportControlsSession,
        Option<EventRegistrationToken>,
    )>,
    changed_ctx: Arc<MediaChangedCallbackContext>,
    session_changed_token: EventRegistrationToken,
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

#[derive(Default)]
struct MediaChangedCallbackContext {
    waker: AtomicWaker,
    media_changed: AtomicBool,
    current_session_changed: AtomicBool,
}

impl WinrtControl {
    pub(super) async fn create() -> io::Result<Self> {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.await?;
        let ctx = Arc::new(MediaChangedCallbackContext {
            waker: AtomicWaker::new(),
            media_changed: AtomicBool::new(false),
            current_session_changed: AtomicBool::new(true),
        });
        let session_changed_token = manager.CurrentSessionChanged(&{
            let ctx = ctx.clone();
            TypedEventHandler::new(move |_, _| {
                ctx.current_session_changed.store(true, Ordering::Release);
                ctx.media_changed.store(true, Ordering::SeqCst);
                ctx.waker.wake();
                Ok(())
            })
        })?;
        let mut me = Self {
            current_session: None,
            is_qqmusic_current: false,
            manager,
            session_changed_token,
            changed_ctx: ctx,
        };
        me.refresh_current_session();
        Ok(me)
    }

    fn refresh_current_session(&mut self) -> Option<GlobalSystemMediaTransportControlsSession> {
        if self
            .changed_ctx
            .current_session_changed
            .swap(false, Ordering::Acquire)
        {
            if let Some((last_session, Some(token))) = self.current_session.take() {
                last_session.RemoveMediaPropertiesChanged(token).ok();
            }
            self.current_session = self.manager.GetCurrentSession().ok().map(|s| (s, None));
            self.is_qqmusic_current = self
                .current_session
                .as_ref()
                .map(|(s, _)| {
                    s.SourceAppUserModelId()
                        .unwrap()
                        .to_string_lossy()
                        .contains("QQMusic.exe")
                })
                .unwrap_or_default();
        }
        self.current_session.as_ref().map(|(s, _)| s.clone())
    }

    pub(super) fn is_qqmusic_current(&mut self) -> bool {
        if self
            .changed_ctx
            .current_session_changed
            .load(Ordering::Relaxed)
        {
            self.refresh_current_session();
        }
        self.is_qqmusic_current
    }

    fn poll_media_change(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.changed_ctx.waker.register(cx.waker());
        if self.changed_ctx.media_changed.swap(false, Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }
        self.refresh_current_session();
        if let Some((session, token @ None)) = &mut self.current_session {
            let ctx = self.changed_ctx.clone();
            *token = session
                .MediaPropertiesChanged(&{
                    TypedEventHandler::new(move |_, _| {
                        ctx.media_changed.store(true, Ordering::SeqCst);
                        ctx.waker.wake();
                        Ok(())
                    })
                })
                .ok();
        }
        Poll::Pending
    }

    pub(super) async fn media_change(&mut self) -> io::Result<()> {
        poll_fn(|cx| self.poll_media_change(cx)).await
    }

    pub(super) async fn get_media_info(&mut self) -> io::Result<Option<WinrtMediaInfo>> {
        let Some(session) = self.refresh_current_session() else {
            return Ok(None);
        };

        let media_property = session.TryGetMediaPropertiesAsync()?.await?;
        let title = media_property.Title()?.to_string_lossy();
        let artist = media_property.Artist()?.to_string_lossy();
        let album = media_property.AlbumTitle()?.to_string_lossy();

        let timeline_property = session.GetTimelineProperties()?;
        let position = timeline_property.Position()?.Duration as _;
        let duration = timeline_property.EndTime()?.Duration as _;

        let paused = session.GetPlaybackInfo()?.PlaybackStatus()?.0 != 4;

        Ok(Some(WinrtMediaInfo {
            title,
            artist,
            album,
            position,
            duration,
            paused,
        }))
    }

    pub(super) async fn get_album_img(&self) -> io::Result<Option<AlbumImage>> {
        use base64::prelude::*;

        let Ok(session) = self.manager.GetCurrentSession() else {
            return Ok(None);
        };
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
        Ok(Some(AlbumImage::Blob {
            mime: content_type,
            base64,
        }))
    }
}

impl Drop for WinrtControl {
    fn drop(&mut self) {
        self.manager
            .RemoveCurrentSessionChanged(self.session_changed_token)
            .ok();
        if let Some((last_session, Some(token))) = self.current_session.take() {
            last_session.RemoveMediaPropertiesChanged(token).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_winrt_control() -> io::Result<()> {
        let mut control = WinrtControl::create().await?;
        let info = control.get_media_info().await?;
        println!("{:#?}", info);
        let img = control.get_album_img().await?;
        println!("{:?}", img);
        Ok(())
    }
}
