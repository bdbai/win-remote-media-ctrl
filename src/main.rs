use std::time::Duration;

use axum::{body::Body, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use hyper::{header::CONTENT_TYPE, Response};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod ctrl;
mod handler;
pub mod media;
mod sys;

async fn load_private_key() -> [u8; 64] {
    use base64::prelude::*;
    let content = tokio::fs::read("private_key.txt")
        .await
        .expect("reading private key");
    let content = BASE64_STANDARD
        .decode(content)
        .expect("decoding private key");
    content
        .as_slice()
        .try_into()
        .expect("converting private key")
}

async fn prepare_rustls_config() -> RustlsConfig {
    use tracing::info;
    async fn download_bytes(url: &str) -> Vec<u8> {
        let bytes = reqwest::get(url).await.unwrap().bytes().await.unwrap();
        bytes.to_vec()
    }
    if let (Some(cert_path), Some(key_path)) = (
        std::env::var_os("WIN_REMOTE_MEDIA_CTRL_TLS_CERT_PATH").filter(|s| !s.is_empty()),
        std::env::var_os("WIN_REMOTE_MEDIA_CTRL_TLS_KEY_PATH").filter(|s| !s.is_empty()),
    ) {
        info!("Using cert and key from env");
        return RustlsConfig::from_pem_chain_file(cert_path, key_path)
            .await
            .unwrap();
    }
    info!("Downloading cert and key from traefik.me");
    let (cert, key) = tokio::join!(
        download_bytes("https://traefik.me/fullchain.pem"),
        download_bytes("https://traefik.me/privkey.pem")
    );
    RustlsConfig::from_pem(cert, key).await.unwrap()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "win_remote_media_ctrl=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let private_key = load_private_key().await;

    let ws_state = handler::ws::WsGlobalState::new(private_key);
    let app = Router::new()
        .route_service("/", ServeFile::new("static/index.html"))
        .nest_service(
            "/static",
            ServeDir::new("static").append_index_html_on_directories(false),
        )
        .layer((
            TraceLayer::new_for_http(),
            TimeoutLayer::new(Duration::from_secs(3)),
            SetResponseHeaderLayer::overriding(CONTENT_TYPE, |res: &Response<Body>| {
                let content_type = res.headers().get(CONTENT_TYPE)?;
                let content_type = content_type.to_str().ok()?;
                if content_type.contains("charset") {
                    return None;
                }
                (content_type.to_owned() + "; charset=utf-8")
                    .try_into()
                    .ok()
            }),
        ))
        .route(
            "/main_ws",
            get(handler::ws::ws_handler).with_state(ws_state),
        );

    axum_server::bind_rustls(
        "0.0.0.0:9201".parse().unwrap(),
        prepare_rustls_config().await,
    )
    .serve(app.into_make_service())
    .await
    .unwrap();
}
