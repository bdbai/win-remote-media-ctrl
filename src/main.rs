use std::time::Duration;

use axum::{body::Body, middleware::from_fn_with_state, routing::post, Router};
use axum_server::tls_rustls::RustlsConfig;
use hyper::{header::CONTENT_TYPE, Response};
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod cmd;
pub mod ctrl;
pub(crate) mod error;

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
    async fn download_bytes(url: &str) -> Vec<u8> {
        let bytes = reqwest::get(url).await.unwrap().bytes().await.unwrap();
        bytes.to_vec()
    }
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

    let auth_state = auth::GlobalAuthState::new(load_private_key().await);
    let cmd_route = Router::new()
        .route("/play_pause", post(cmd::handle_play_pause))
        .route("/next_track", post(cmd::handle_next_track))
        .route("/prev_track", post(cmd::handle_prev_track))
        .route("/volume_down", post(cmd::handle_volume_down))
        .route("/volume_up", post(cmd::handle_volume_up))
        .layer(from_fn_with_state(
            auth_state.clone(),
            auth::auth_middleware_fn,
        ));

    let app = Router::new()
        .route_service("/", ServeFile::new("static/index.html"))
        .nest_service(
            "/static",
            ServeDir::new("static").append_index_html_on_directories(false),
        )
        .route("/session", post(auth::handle_new_session))
        .nest("/cmd", cmd_route)
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
        .with_state(auth_state);

    axum_server::bind_rustls(
        "0.0.0.0:9201".parse().unwrap(),
        prepare_rustls_config().await,
    )
    .serve(app.into_make_service())
    .await
    .unwrap();
}
