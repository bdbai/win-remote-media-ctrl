use std::time::Duration;

use aes_gcm::{Aes128Gcm, KeyInit};
use axum::{extract::State, response::IntoResponse, Json};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::error::ResponseError;
pub use crate::sys::{get_album_image, get_media_info, VolumeClient};

#[derive(Clone)]
pub struct MediaCryptoState {
    _aes: Aes128Gcm,
}

impl MediaCryptoState {
    pub fn new(key: &[u8]) -> Self {
        Self {
            _aes: Aes128Gcm::new(key[..16].into()),
        }
    }
}

pub(crate) async fn handle_get_media_info(
    State(_crypto): State<MediaCryptoState>,
) -> impl IntoResponse {
    let media_info = match get_media_info().await {
        Ok(info) => info,
        Err(err) => {
            error!(?err, "Failed to get media info");
            return Err(ResponseError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: "get_media_info_error",
            });
        }
    };
    // TODO: proper encryption
    Ok(Json(media_info))
}

pub(crate) async fn handle_get_album_image(
    State(_crypto): State<MediaCryptoState>,
) -> impl IntoResponse {
    let image = match get_album_image().await {
        Ok(Some(image)) => image,
        Ok(None) => {
            return Err(ResponseError {
                status_code: StatusCode::NOT_FOUND,
                error_code: "no_album_image",
            });
        }
        Err(err) => {
            error!(?err, "Failed to get album image");
            return Err(ResponseError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: "get_album_image_error",
            });
        }
    };
    // TODO: proper encryption
    Ok(Json(image))
}

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
