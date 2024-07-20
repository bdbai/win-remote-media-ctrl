use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of_val,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hmac::{Mac, SimpleHmac};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{debug, info, warn};

use crate::error::ResponseError;

#[derive(Clone)]
pub(crate) struct GlobalAuthState {
    private_key_hmac: SimpleHmac<Sha256>,
    sessions: Arc<Mutex<GlobalAuthSessionRegistry>>,
}

impl GlobalAuthState {
    pub fn new(private_key: [u8; 64]) -> Self {
        Self {
            private_key_hmac: SimpleHmac::new(&private_key.into()),
            sessions: Arc::new(Mutex::new(GlobalAuthSessionRegistry::default())),
        }
    }
}

#[derive(Default)]
struct GlobalAuthSessionRegistry {
    sessions: BTreeMap<SessionId, AuthSession>,
    last_timestamp: u64,
}

#[derive(Debug, Clone)]
struct AuthSession {
    created_at: Instant,
    seed: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
struct SessionId {
    #[serde(serialize_with = "serialize_b64", deserialize_with = "deserialize_b64")]
    id: [u8; 16],
}

pub(crate) async fn handle_new_session(
    State(state): State<GlobalAuthState>,
    Json(req): Json<NewSessionRequest>,
) -> Result<Json<NewSessionResponse>, ResponseError> {
    const MAX_TIMESTAMP_DISCREPANCY: u64 = 1000 * 60;
    const MAX_SESSION: usize = 9;

    let mut mac = state.private_key_hmac.clone();
    mac.update(&req.timestamp);
    mac.verify_slice(&req.auth).map_err(|_| {
        warn!("bad auth");
        ResponseError {
            status_code: StatusCode::UNAUTHORIZED,
            error_code: "bad_auth",
        }
    })?;
    let req_timestamp = u64::from_be_bytes(req.timestamp);

    let now = Instant::now();
    let now_unix = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as u64;
    let oldest_session_time = now - Duration::from_secs(60 * 60 * 24);

    let mut registry = state.sessions.lock().unwrap();
    if req_timestamp <= registry.last_timestamp {
        warn!(
            "outdated timestamp: {} <= {}",
            req_timestamp, registry.last_timestamp
        );
        return Err(ResponseError {
            status_code: StatusCode::BAD_REQUEST,
            error_code: "outdated_timestamp",
        });
    }
    if req_timestamp.abs_diff(now_unix) > MAX_TIMESTAMP_DISCREPANCY {
        warn!("bad timestamp: {} (now: {})", req_timestamp, now_unix);
        return Err(ResponseError {
            status_code: StatusCode::BAD_REQUEST,
            error_code: "bad_timestamp",
        });
    }

    let existing_session_count = registry.sessions.len();
    registry
        .sessions
        .retain(|_, session| session.created_at > oldest_session_time);
    if let Some(excessive_session_count) = registry.sessions.len().checked_sub(MAX_SESSION) {
        let sorted_sessions = registry
            .sessions
            .iter()
            .map(|(&key, value)| (value.created_at, key))
            .collect::<BTreeSet<_>>();
        for (_, key) in sorted_sessions.into_iter().take(excessive_session_count) {
            registry.sessions.remove(&key);
        }
    }
    debug!(
        "removed {} sessions, {} remaining",
        existing_session_count - registry.sessions.len(),
        registry.sessions.len()
    );
    registry.last_timestamp = now_unix;
    let mut new_id_key = [0u8; 16 + 64];
    getrandom::getrandom(&mut new_id_key).expect("generating random key");
    let id = SessionId {
        id: new_id_key[..16].try_into().unwrap(),
    };
    let seed = new_id_key[16..].try_into().unwrap();
    registry.sessions.insert(
        id,
        AuthSession {
            created_at: now,
            seed,
        },
    );
    drop(registry);

    info!("new session");
    Ok(Json(NewSessionResponse {
        id,
        seed: new_id_key[16..].try_into().unwrap(),
    }))
}

pub(crate) async fn auth_middleware_fn(
    State(state): State<GlobalAuthState>,
    request: Request,
    next: Next,
) -> Response {
    use base64::prelude::*;

    let Some(session_verify_value) = request.headers().get("session-verify") else {
        warn!("received a request without session-verify header");
        return ResponseError {
            status_code: StatusCode::UNAUTHORIZED,
            error_code: "no_session_verify_header",
        }
        .into_response();
    };
    let mut session_verify_bytes = [0u8; 48];
    let Ok(decoded_len) =
        BASE64_STANDARD.decode_slice(session_verify_value.as_bytes(), &mut session_verify_bytes)
    else {
        warn!("bad session-verify base64");
        return ResponseError {
            status_code: StatusCode::BAD_REQUEST,
            error_code: "invalid_session_verify_header",
        }
        .into_response();
    };
    if decoded_len != size_of_val(&session_verify_bytes) {
        warn!("bad session-verify length");
        return ResponseError {
            status_code: StatusCode::BAD_REQUEST,
            error_code: "invalid_session_verify_header",
        }
        .into_response();
    }
    let session_id = SessionId {
        id: session_verify_bytes[..16].try_into().unwrap(),
    };
    let given_auth: [u8; 32] = session_verify_bytes[16..].try_into().unwrap();

    let mut mac = state.private_key_hmac.clone();
    let verify_result = {
        let mut registry = state.sessions.lock().unwrap();
        let Some(session) = registry.sessions.get_mut(&session_id) else {
            warn!("bad session id");
            return ResponseError {
                status_code: StatusCode::UNAUTHORIZED,
                error_code: "bad_session_id",
            }
            .into_response();
        };
        mac.update(&session.seed);
        {
            let mut c = 1;
            for i in 0..session.seed.len() {
                c += session.seed[i] as u16;
                session.seed[i] = c as u8;
                c >>= 8;
            }
        }
        mac.verify_slice(&given_auth)
    };
    if verify_result.is_err() {
        warn!("bad auth");
        return ResponseError {
            status_code: StatusCode::UNAUTHORIZED,
            error_code: "bad_auth",
        }
        .into_response();
    }

    next.run(request).await
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NewSessionRequest {
    #[serde(deserialize_with = "deserialize_b64")]
    timestamp: [u8; 8],
    #[serde(deserialize_with = "deserialize_b64")]
    auth: [u8; 32],
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NewSessionResponse {
    id: SessionId,
    #[serde(serialize_with = "serialize_b64")]
    seed: [u8; 64],
}

fn deserialize_b64<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
where
    D: serde::Deserializer<'de>,
{
    use std::borrow::Cow;

    use base64::prelude::*;
    use serde::de::Error;

    let s = Cow::<str>::deserialize(deserializer)?;
    let mut bytes = [0u8; N];
    let count = BASE64_STANDARD
        .decode_slice(s.as_bytes(), &mut bytes)
        .map_err(D::Error::custom)?;
    if count != N {
        return Err(D::Error::custom("invalid length"));
    }
    Ok(bytes)
}

fn serialize_b64<S>(key: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use base64::prelude::*;
    serializer.serialize_str(&BASE64_STANDARD.encode(key))
}
