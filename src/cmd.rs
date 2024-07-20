use std::io;

use axum::{response::IntoResponse, Json};
use hyper::StatusCode;
use serde::Serialize;
use tracing::{error, info};

use crate::{ctrl, error::ResponseError};

fn handle_cmd(cmd: impl Fn() -> io::Result<()>, name: &str) -> impl IntoResponse {
    info!(%name, "handling command");
    match cmd() {
        Ok(_) => Ok(Json(CommandResponse {})),
        Err(err) => {
            error!(?err, "Failed to handle command {}", name);
            Err(ResponseError {
                status_code: StatusCode::INTERNAL_SERVER_ERROR,
                error_code: "ctrl_error",
            })
        }
    }
}

pub(crate) async fn handle_play_pause() -> impl IntoResponse {
    handle_cmd(ctrl::press_play_pause, "play_pause")
}

pub(crate) async fn handle_next_track() -> impl IntoResponse {
    handle_cmd(ctrl::press_next_track, "next_track")
}

pub(crate) async fn handle_prev_track() -> impl IntoResponse {
    handle_cmd(ctrl::press_prev_track, "prev_track")
}

pub(crate) async fn handle_volume_down() -> impl IntoResponse {
    handle_cmd(ctrl::press_volume_down, "volume_down")
}

pub(crate) async fn handle_volume_up() -> impl IntoResponse {
    handle_cmd(ctrl::press_volume_up, "volume_up")
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandResponse {}
