use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponseError {
    #[serde(skip)]
    pub(crate) status_code: StatusCode,
    #[serde(rename = "code")]
    pub(crate) error_code: &'static str,
}

impl IntoResponse for ResponseError {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&self).expect("serializing error response");
        (self.status_code, body).into_response()
    }
}
