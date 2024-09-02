use std::time::Duration;
use std::{io, pin::pin};

use axum::{
    extract::ws::{self, WebSocket},
    Error as AxumError,
};
use serde::{Deserialize, Serialize};
use tokio::{
    select,
    time::{interval, sleep, timeout, Instant},
};
use tracing::{error, warn};

use super::crypto::Crypto;
use super::WebSocketResult;
use crate::handler::ws::error::WebSocketError;
use crate::media::{AlbumImage, MediaManager, TimelineState, VolumeState};

pub(super) async fn handle_socket_inner(
    ws: &mut WebSocket,
    crypto: &mut Crypto,
) -> WebSocketResult<()> {
    const HEARTBEAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
    const SESSION_TOTAL_TIMEOUT: Duration = Duration::from_secs(3600 * 8);
    const ALBUM_TIMEOUT: Duration = Duration::from_secs(1);

    match timeout(Duration::from_secs(5), initial_heartbeat(ws, crypto)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            warn!("websocket handshake timeout");
            return Err(WebSocketError {
                close_frame: ws::CloseFrame {
                    code: 3001,
                    reason: "handshake timeout".into(),
                },
                error: io::Error::new(io::ErrorKind::TimedOut, "handshake timeout").into(),
            });
        }
    }

    let mut heartbeat_interval = interval(Duration::from_secs(35));
    heartbeat_interval.reset();
    let mut heartbeat_timeout = pin!(sleep(SESSION_TOTAL_TIMEOUT));
    let mut timeline_interval = interval(Duration::from_secs(1));
    let mut media_interval = interval(Duration::from_secs(5));
    media_interval.reset();
    let mut album_retry = 0;
    let mut album_timeout = pin!(sleep(SESSION_TOTAL_TIMEOUT));
    let mut media_manager = MediaManager::new().await?;
    let mut last_album_hash = "".into();
    let mut last_track = {
        let media_info = media_manager.get_media_info().await;
        if let Ok(Some(_)) = media_info {
            album_timeout.as_mut().reset(Instant::now());
        }
        let media_info = media_info.ok().flatten().unwrap_or_default();
        let content = serde_json::to_string(&media_info).unwrap();
        ws.send(ws::Message::Binary(crypto.encrypt(content.as_bytes())))
            .await?;
        media_info.track
    };
    let mut last_timeline = media_manager.get_timeline_state().await.unwrap_or_default();
    let mut volume_client = crate::media::VolumeClient::create()?;
    loop {
        enum TriggerType {
            Heartbeat,
            MediaChanged,
            VolumeChanged,
            TimelineInterval,
            AlbumTimeout,
            Recv(Option<Result<ws::Message, AxumError>>),
        }
        let mut trigger_type = select! {
            _ = heartbeat_interval.tick() => { TriggerType::Heartbeat },
            _ = heartbeat_timeout.as_mut() => {
                // Future is not fused. Return immediately to prevent polling again.
                error!("websocket heartbeat timeout");
                break Ok(());
            },
            res = media_manager.media_change() => { res?; TriggerType::MediaChanged },
            _ = volume_client.volume_change() => { TriggerType::VolumeChanged },
            _ = timeline_interval.tick() => { TriggerType::TimelineInterval },
            _ = media_interval.tick() => { TriggerType::MediaChanged },
            _ = album_timeout.as_mut() => { TriggerType::AlbumTimeout },
            msg = ws.recv() => { TriggerType::Recv(msg) },
        };
        trigger_type = match trigger_type {
            TriggerType::Recv(recv) => match recv {
                Some(Ok(ws::Message::Close(_))) | None => break Ok(()),
                Some(Err(e)) => {
                    error!("websocket recv error: {:?}", e);
                    break Err(e.into());
                }
                Some(Ok(msg)) => {
                    heartbeat_interval.reset();
                    heartbeat_timeout
                        .as_mut()
                        .reset(Instant::now() + SESSION_TOTAL_TIMEOUT);
                    let req: Request = {
                        let mut msg = msg.into_data();
                        let data = crypto.decrypt_in_place(&mut msg)?;
                        serde_json::from_slice(data)?
                    };
                    use crate::ctrl::*;
                    let res = match &req {
                        Request::Heartbeat => {
                            let res = serde_json::to_string(&HeartbeatRes::default()).unwrap();
                            ws.send(ws::Message::Binary(crypto.encrypt(res.as_bytes())))
                                .await?;
                            Ok(())
                        }
                        Request::HeartbeatRes => Ok(()),
                        Request::TogglePlayPause => press_play_pause(),
                        Request::NextTrack => press_next_track(),
                        Request::PrevTrack => press_prev_track(),
                        Request::VolumeDown => press_volume_down(),
                        Request::VolumeUp => press_volume_up(),
                        Request::Like => press_like(),
                    };
                    if let Err(e) = res {
                        error!("failed to handle command {:?}: {:?}", req, e);
                    }
                    match req {
                        Request::TogglePlayPause => TriggerType::TimelineInterval,
                        _ => continue,
                    }
                }
            },
            t => t,
        };
        let content = match trigger_type {
            TriggerType::Heartbeat => {
                heartbeat_timeout
                    .as_mut()
                    .reset(Instant::now() + HEARTBEAT_REQUEST_TIMEOUT);
                serde_json::to_string(&Heartbeat::default()).unwrap()
            }
            TriggerType::MediaChanged => match media_manager.get_media_info().await {
                Ok(media_info) => {
                    let media_info = media_info.unwrap_or_default();
                    let content = serde_json::to_string(&media_info).unwrap();
                    let track = media_info.track;
                    if track == last_track {
                        continue;
                    }
                    if track.album != last_track.album {
                        album_retry = 0;
                        album_timeout.as_mut().reset(Instant::now());
                    }
                    last_track = track;
                    content
                }
                Err(e) => serde_json::to_string(&ErrorRes {
                    ctx: "media_changed",
                    error: e.to_string(),
                })
                .unwrap(),
            },
            TriggerType::VolumeChanged => match volume_client.get_volume() {
                Ok(volume) => {
                    let volume = VolumeRes { volume };
                    serde_json::to_string(&volume).unwrap()
                }
                Err(e) => serde_json::to_string(&ErrorRes {
                    ctx: "volume_changed",
                    error: e.to_string(),
                })
                .unwrap(),
            },
            TriggerType::TimelineInterval => match media_manager.get_timeline_state().await {
                Ok(timeline_state) => {
                    if timeline_state == last_timeline {
                        continue;
                    }
                    let content = serde_json::to_string(&TimelineRes {
                        timeline: timeline_state.as_ref().unwrap_or(&TimelineState::default()),
                    })
                    .unwrap();
                    last_timeline = timeline_state;
                    content
                }
                Err(e) => serde_json::to_string(&ErrorRes {
                    ctx: "timeline_interval",
                    error: e.to_string(),
                })
                .unwrap(),
            },
            TriggerType::AlbumTimeout => {
                // Make sure this future has been reset before polling again.
                album_timeout
                    .as_mut()
                    .reset(Instant::now() + SESSION_TOTAL_TIMEOUT);
                match media_manager.get_album_image().await {
                    Ok(Some(album)) => {
                        let hash = calculate_album_hash(&album);
                        if let Some(album) = Some(&album).filter(|_| hash != last_album_hash) {
                            last_album_hash = hash.to_owned();
                            serde_json::to_string(&AlbumRes {
                                album_img: Some(album),
                            })
                            .unwrap()
                        } else {
                            album_retry += 1;
                            if album_retry < 10 {
                                album_timeout.as_mut().reset(Instant::now() + ALBUM_TIMEOUT);
                            } else {
                                warn!("failed to get album image after 3 retries");
                            }
                            continue;
                        }
                    }
                    Ok(None) => serde_json::to_string(&AlbumRes { album_img: None }).unwrap(),
                    Err(e) => serde_json::to_string(&ErrorRes {
                        ctx: "album_timeout",
                        error: e.to_string(),
                    })
                    .unwrap(),
                }
            }
            TriggerType::Recv(_) => continue,
        };
        ws.send(ws::Message::Binary(crypto.encrypt(content.as_bytes())))
            .await?;
    }
}

async fn initial_heartbeat(ws: &mut WebSocket, crypto: &mut Crypto) -> WebSocketResult<()> {
    let req = {
        let msg = ws.recv().await;
        let Some(Ok(msg)) = msg else {
            error!("expecting initial heartbeat msg, but got {:?}", msg);
            return Err(WebSocketError {
                close_frame: ws::CloseFrame {
                    code: 3001,
                    reason: "unexpected eof".into(),
                },
                error: io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected eof").into(),
            });
        };
        let mut msg = msg.into_data();
        let msg = crypto.decrypt_in_place(&mut msg)?;
        serde_json::from_slice::<Request>(msg)?
    };
    (req == Request::Heartbeat).then_some(()).ok_or_else(|| {
        warn!("expecting initial heartbeat, but got {:?}", req);
        WebSocketError {
            close_frame: ws::CloseFrame {
                code: 3001,
                reason: "expecting initial heartbeat".into(),
            },
            error: io::Error::new(io::ErrorKind::InvalidData, "expecting initial handshake").into(),
        }
    })?;
    let res = serde_json::to_string(&HeartbeatRes::default()).unwrap();
    ws.send(ws::Message::Binary(crypto.encrypt(res.as_bytes())))
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum Request {
    Heartbeat,
    HeartbeatRes,
    TogglePlayPause,
    NextTrack,
    PrevTrack,
    VolumeDown,
    VolumeUp,
    Like,
}

#[derive(Debug, Clone, Default, Serialize)]
struct Heartbeat {
    heartbeat: (),
}

#[derive(Debug, Clone, Default, Serialize)]
struct HeartbeatRes {
    heartbeat_res: (),
}

#[derive(Debug, Clone, Serialize)]
struct ErrorRes {
    ctx: &'static str,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
struct AlbumRes<'a> {
    album_img: Option<&'a AlbumImage>,
}

#[derive(Debug, Clone, Serialize)]
struct TimelineRes<'a> {
    timeline: &'a TimelineState,
}

fn calculate_album_hash(album: &AlbumImage) -> &str {
    match album {
        AlbumImage::Url(url) => url,
        AlbumImage::Blob { base64, .. } => base64,
    }
}

#[derive(Debug, Clone, Serialize)]
struct VolumeRes {
    volume: VolumeState,
}
