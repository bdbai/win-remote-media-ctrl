use std::{error::Error, io};

use axum::extract::ws;

#[derive(Debug)]
pub(super) struct WebSocketError {
    pub(super) close_frame: ws::CloseFrame<'static>,
    pub(super) error: Box<dyn Error>,
}
pub(super) type WebSocketResult<T> = Result<T, WebSocketError>;

impl From<axum::Error> for WebSocketError {
    fn from(error: axum::Error) -> Self {
        Self {
            close_frame: ws::CloseFrame {
                code: 3000,
                reason: error.to_string().into(),
            },
            error: error.into(),
        }
    }
}
impl From<io::Error> for WebSocketError {
    fn from(error: io::Error) -> Self {
        Self {
            close_frame: ws::CloseFrame {
                code: 3001,
                reason: error.to_string().into(),
            },
            error: error.into(),
        }
    }
}
impl From<serde_json::Error> for WebSocketError {
    fn from(error: serde_json::Error) -> Self {
        Self {
            close_frame: ws::CloseFrame {
                code: 3002,
                reason: error.to_string().into(),
            },
            error: error.into(),
        }
    }
}
