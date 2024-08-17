use std::io;

use axum::{response::IntoResponse, Json};
use hyper::StatusCode;
use paste::paste;
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

macro_rules! handle_cmd {
    ($name:ident) => {
        paste! {
            pub(crate) async fn [<handle_ $name>]() -> impl IntoResponse {
                handle_cmd(ctrl::[<press_ $name>], stringify!($name))
            }
        }
    };
}

handle_cmd!(play_pause);
handle_cmd!(next_track);
handle_cmd!(prev_track);
handle_cmd!(volume_down);
handle_cmd!(volume_up);
handle_cmd!(like);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandResponse {}
