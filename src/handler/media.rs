use std::sync::{LazyLock, Mutex};

use aes_gcm::{Aes128Gcm, KeyInit};
use axum::{extract::State, response::IntoResponse, Json};
use hyper::StatusCode;
use tracing::error;

use crate::error::ResponseError;
use crate::sys::{get_album_image, get_media_info, VolumeClient};

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

pub(crate) async fn handle_get_volume() -> impl IntoResponse {
    static VOLUME_CLIENT: LazyLock<Mutex<VolumeClient>> = LazyLock::new(|| {
        Mutex::new(VolumeClient::create().expect("Failed to create volume client"))
    });

    let mut volume_client = VOLUME_CLIENT.lock().unwrap();
    let volume = match volume_client.get_volume() {
        Ok(volume) => volume,
        Err(err) => {
            error!(?err, "Failed to get volume");
            return Err(ResponseError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: "get_volume_error",
            });
        }
    };
    Ok(Json(volume))
}
