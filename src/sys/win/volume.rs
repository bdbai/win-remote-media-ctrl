use std::future::poll_fn;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::task::AtomicWaker;
use tracing::info;
use windows::core::{implement, AsImpl, Result};
use windows::Win32::Media::Audio::{
    eMultimedia, eRender,
    Endpoints::{
        IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
    },
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::media::VolumeState;

static COM_INIT: std::sync::Once = std::sync::Once::new();

pub struct VolumeClient {
    volume: IAudioEndpointVolume,
    callback: Option<IAudioEndpointVolumeCallback>,
}

#[implement(IAudioEndpointVolumeCallback)]
#[derive(Default)]
struct VolumeChangedCallback {
    waker: AtomicWaker,
    fired: AtomicBool,
}

impl VolumeClient {
    fn get_default_audio_volume() -> Result<IAudioEndpointVolume> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)?;
            let endpoint = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let volume: IAudioEndpointVolume = endpoint.Activate(CLSCTX_INPROC_SERVER, None)?;
            Ok(volume)
        }
    }
    pub fn create() -> io::Result<Self> {
        COM_INIT.call_once(|| unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        });
        Ok(Self {
            volume: Self::get_default_audio_volume()?,
            callback: None,
        })
    }

    fn reset_audio_volume(&mut self) -> Result<()> {
        self.volume = Self::get_default_audio_volume()?;
        if let Some(callback) = &self.callback {
            unsafe {
                self.volume.RegisterControlChangeNotify(callback)?;
            }
        }
        Ok(())
    }
    fn get_volume_once(&self) -> Result<VolumeState> {
        unsafe {
            let level = (self.volume.GetMasterVolumeLevelScalar()? * 100.0).round() / 100.0;
            let muted = self.volume.GetMute()?.0 != 0;
            Ok(VolumeState { level, muted })
        }
    }
    pub fn get_volume(&mut self) -> io::Result<VolumeState> {
        let mut res = self.get_volume_once();
        while res.is_err() {
            info!("audio volume reset");
            self.reset_audio_volume()?;
            res = self.get_volume_once();
        }
        Ok(res?)
    }

    pub fn poll_volume_change(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let volume = &self.volume;
        let callback = if let Some(callback) = &self.callback {
            callback
        } else {
            let callback: IAudioEndpointVolumeCallback = VolumeChangedCallback::default().into();
            unsafe {
                volume.RegisterControlChangeNotify(&callback)?;
            }
            self.callback.insert(callback)
        };
        let callback_impl: &VolumeChangedCallback = unsafe { callback.as_impl() };
        if callback_impl.fired.swap(false, Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            callback_impl.waker.register(cx.waker());
            Poll::Pending
        }
    }
    pub async fn volume_change(&mut self) -> io::Result<()> {
        poll_fn(|cx| self.poll_volume_change(cx)).await
    }
}

impl Drop for VolumeClient {
    fn drop(&mut self) {
        if let Some(callback) = &self.callback {
            unsafe {
                let _ = self.volume.UnregisterControlChangeNotify(callback);
            }
        }
    }
}

impl IAudioEndpointVolumeCallback_Impl for VolumeChangedCallback {
    fn OnNotify(&self, _pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        self.fired.store(true, Ordering::SeqCst);
        self.waker.wake();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::time::{error::Elapsed, timeout, Duration};

    #[tokio::test]
    async fn test_client() -> io::Result<()> {
        let mut client = VolumeClient::create()?;
        let state = client.get_volume()?;
        eprintln!("init state: {:?}", state);
        let res: std::result::Result<io::Result<()>, Elapsed> =
            timeout(Duration::from_secs(5), async {
                loop {
                    client.volume_change().await?;
                    let state = client.get_volume().unwrap();
                    eprintln!("changed state: {:?}", state);
                }
            })
            .await;
        drop(res);
        Ok(())
    }
}
