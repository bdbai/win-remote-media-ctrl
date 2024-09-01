use std::io;
use std::time::Duration;

use aes_gcm::{Aes128Gcm, Key as AesGcmKey};
use axum::extract::ws::{self, WebSocket};
use p256::elliptic_curve::sec1::FromEncodedPoint;
use p256::elliptic_curve::subtle::CtOption;
use p256::{ecdh::EphemeralSecret, EncodedPoint, PublicKey};
use rand::rngs::OsRng;
use sha2::Sha256;
use tokio::time::timeout;
use tracing::error;

use super::{crypto::Crypto, WebSocketError, WebSocketResult};

type PreSharedKey = [u8; 64];
type Aes128GcmKey = AesGcmKey<Aes128Gcm>;

struct NegotiateResult {
    upload_key: Aes128GcmKey,
    download_key: Aes128GcmKey,
}

pub(super) async fn negotiate_ws(
    ws: &mut WebSocket,
    psk: &PreSharedKey,
) -> WebSocketResult<Crypto> {
    let res = timeout(Duration::from_secs(5), negotiate_ws_inner(ws, psk)).await;
    match res {
        Ok(Ok(res)) => Ok(Crypto::new(&res.upload_key, &res.download_key)),
        Ok(Err(e)) => {
            error!("websocket negotiate error: {:#?}", e);
            Err(e)
        }
        Err(_) => {
            error!("websocket negotiation timeout");
            Err(WebSocketError {
                close_frame: ws::CloseFrame {
                    code: 3003,
                    reason: "negotiation timeout".into(),
                },
                error: io::Error::new(io::ErrorKind::TimedOut, "negotiation timeout").into(),
            })
        }
    }
}

async fn negotiate_ws_inner(
    ws: &mut WebSocket,
    psk: &PreSharedKey,
) -> WebSocketResult<NegotiateResult> {
    let client_material = {
        let Some(client_material) = ws.recv().await else {
            return Err(WebSocketError {
                close_frame: ws::CloseFrame {
                    code: 3002,
                    reason: "unexpected eof".into(),
                },
                error: io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected eof").into(),
            });
        };
        let client_material = client_material?.into_data();
        let Ok(Some(client_material)) = EncodedPoint::from_bytes(client_material)
            .as_ref()
            .map(PublicKey::from_encoded_point)
            .map(CtOption::into_option)
        else {
            return Err(WebSocketError {
                close_frame: ws::CloseFrame {
                    code: 1003,
                    reason: "invalid client material".into(),
                },
                error: io::Error::new(io::ErrorKind::InvalidData, "invalid client material").into(),
            });
        };
        client_material
    };

    let server_material = EphemeralSecret::random(&mut OsRng);
    {
        let server_material = EncodedPoint::from(&server_material.public_key());
        ws.send(ws::Message::Binary(server_material.as_bytes().to_vec()))
            .await?;
    }
    let secret = server_material.diffie_hellman(&client_material);
    let hkdf = secret.extract::<Sha256>(Some(psk));
    let mut upload_key = Aes128GcmKey::default();
    let mut download_key = Aes128GcmKey::default();
    hkdf.expand(b"upload", &mut upload_key)
        .expect("hkdf okm too large");
    hkdf.expand(b"download", &mut download_key)
        .expect("hkdf okm too large");
    Ok(NegotiateResult {
        upload_key,
        download_key,
    })
}
