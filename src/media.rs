use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use crate::sys::{get_album_image, get_media_info, VolumeClient};

#[derive(Debug, Clone, Serialize)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub timeline: TimelineState,
}

#[derive(Debug, Clone, Serialize)]
pub enum AlbumImage {
    Url(String),
    Blob { mime: String, base64: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TimelineState {
    #[serde(serialize_with = "serialize_ms")]
    pub duration: Duration,
    #[serde(serialize_with = "serialize_ms")]
    pub position: Duration,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VolumeState {
    pub level: f32,
    pub muted: bool,
}

fn serialize_ms<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u64(duration.as_millis() as u64)
}
