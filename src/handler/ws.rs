use std::sync::Arc;

use axum::extract::{
    ws::{self, WebSocket},
    State, WebSocketUpgrade,
};
use axum::response::IntoResponse;
use tracing::{debug, error};

mod auth;
mod crypto;
mod error;
mod r#loop;

use error::{WebSocketError, WebSocketResult};

#[derive(Clone)]
pub struct WsGlobalState {
    psk: Arc<[u8; 64]>,
}

impl WsGlobalState {
    pub fn new(psk: [u8; 64]) -> Self {
        Self { psk: Arc::new(psk) }
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    // ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<WsGlobalState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut ws: WebSocket, state: WsGlobalState) {
    let close_frame = {
        let ws_res = handle_socket_inner(&mut ws, &state).await;
        if let Err(e) = &ws_res {
            error!("websocket error: {:#?}", e.error);
        }
        ws_res.err().map(|e| e.close_frame)
    };
    if let Err(e) = ws.send(ws::Message::Close(close_frame)).await {
        debug!("failed to close websocket: {}", e);
    }
}

async fn handle_socket_inner(ws: &mut WebSocket, state: &WsGlobalState) -> WebSocketResult<()> {
    let mut crypto = auth::negotiate_ws(ws, &state.psk).await?;
    r#loop::handle_socket_inner(ws, &mut crypto).await
}
