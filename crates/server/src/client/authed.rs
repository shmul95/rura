use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::{get_user_pubkey, set_user_pubkey};
use crate::utils::debug::debug_io_enabled;
use crate::webrtc;
use rusqlite::Connection;

fn emit_error(outbound: &mpsc::UnboundedSender<ClientMessage>, msg: &str) {
    let _ = outbound.send(ClientMessage {
        command: "error".to_string(),
        data: msg.to_string(),
    });
}

fn require_session_id(
    session_user_id: Option<i64>,
    outbound: &mpsc::UnboundedSender<ClientMessage>,
) -> Option<i64> {
    if let Some(id) = session_user_id {
        Some(id)
    } else {
        emit_error(outbound, "Missing session identity; reconnect required");
        None
    }
}

pub(super) async fn handle_client_message(
    state: Arc<AppState>,
    conn: Arc<Mutex<Connection>>,
    outbound: &mpsc::UnboundedSender<ClientMessage>,
    client_addr: SocketAddr,
    user_id: String,
    buffer: &[u8],
) -> tokio::io::Result<()> {
    let received = String::from_utf8_lossy(buffer).to_string();
    match serde_json::from_str::<ClientMessage>(&received) {
        Ok(msg) => {
            let session_user_id = webrtc::handler::identity_to_session_id(&user_id);
            if debug_io_enabled() {
                // Avoid logging message bodies; only log safe signaling metadata.
                if msg.command == "message" {
                    println!(
                        "<<< [{} {}] message <redacted>, data_len={}",
                        client_addr,
                        user_id,
                        msg.data.len()
                    );
                } else if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&msg.data) {
                    // If this is an RTC message with an SDP string that itself contains JSON,
                    // attempt to parse it so we don't print escaped content.
                    if let Some(sdp_str) = v.get("sdp").and_then(|x| x.as_str())
                        && let Ok(inner) = serde_json::from_str::<serde_json::Value>(sdp_str)
                        && let Some(slot) = v.get_mut("sdp")
                    {
                        *slot = inner;
                    }
                    println!("<<< [{} {}] {} {}", client_addr, user_id, msg.command, v);
                } else {
                    println!(
                        "<<< [{} {}] {} (len={})",
                        client_addr,
                        user_id,
                        msg.command,
                        msg.data.len()
                    );
                }
            } else {
                // Keep concise log by default; do not print message bodies
                println!(
                    "Received cmd '{}' from user {} ({}), data_len={}",
                    msg.command,
                    user_id,
                    client_addr,
                    msg.data.len()
                );
            }
            match msg.command.as_str() {
                "message" => {
                    #[derive(serde::Deserialize)]
                    struct LocalDM {
                        body: String,
                    }
                    fn is_base64ish(s: &str) -> bool {
                        !s.is_empty()
                            && s.chars().all(|c| {
                                matches!(
                                    c,
                                    'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' | '-' | '_' // allow URL-safe too
                                )
                            })
                    }
                    fn is_e2ee_envelope(body: &str) -> bool {
                        if !body.starts_with("v1:") {
                            return false;
                        }
                        let parts: Vec<&str> = body.split(':').collect();
                        if parts.len() != 4 {
                            return false;
                        }
                        let (_v, eph, nonce, ct) = (parts[0], parts[1], parts[2], parts[3]);
                        is_base64ish(eph) && is_base64ish(nonce) && is_base64ish(ct)
                    }
                    match serde_json::from_str::<LocalDM>(&msg.data) {
                        Ok(req) => {
                            if state.require_e2ee() && !is_e2ee_envelope(&req.body) {
                                let err = ClientMessage {
                                    command: "error".to_string(),
                                    data: "E2EE required: invalid or missing envelope".to_string(),
                                };
                                let _ = outbound.send(err);
                                return Ok(());
                            }
                            // Do not relay message bodies; instruct client to use WebRTC channel.
                            let _ = req; // avoid unused warnings
                            let info = ClientMessage {
                                command: "error".to_string(),
                                data: "Message relay disabled: use WebRTC".to_string(),
                            };
                            let _ = outbound.send(info);
                        }
                        Err(_) => {
                            // Notify sender about malformed message request
                            let err = ClientMessage {
                                command: "error".to_string(),
                                data: "Invalid message format".to_string(),
                            };
                            let _ = outbound.send(err);
                        }
                    }
                }
                "rtc_offer" => {
                    match serde_json::from_str::<rura_models::webrtc::RtcOffer>(&msg.data) {
                        Ok(mut offer) => {
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            offer.from_user_id = from_id;
                            if let Err(err) = webrtc::process_offer(Arc::clone(&state), offer).await
                            {
                                emit_error(outbound, &format!("rtc_offer rejected: {err}"));
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid rtc_offer format");
                        }
                    }
                }
                "rtc_answer" => {
                    match serde_json::from_str::<rura_models::webrtc::RtcAnswer>(&msg.data) {
                        Ok(mut answer) => {
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            answer.from_user_id = from_id;
                            if let Err(err) =
                                webrtc::process_answer(Arc::clone(&state), answer).await
                            {
                                emit_error(outbound, &format!("rtc_answer rejected: {err}"));
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid rtc_answer format");
                        }
                    }
                }
                "rtc_ice" => {
                    match serde_json::from_str::<rura_models::webrtc::IceCandidate>(&msg.data) {
                        Ok(mut ice) => {
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            ice.from_user_id = from_id;
                            if let Err(err) = webrtc::process_ice(Arc::clone(&state), ice).await {
                                emit_error(outbound, &format!("rtc_ice rejected: {err}"));
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid rtc_ice format");
                        }
                    }
                }
                "call_invite" => {
                    match serde_json::from_str::<rura_models::webrtc::CallInvite>(&msg.data) {
                        Ok(mut invite) => {
                            if invite.call_id.trim().is_empty() {
                                emit_error(outbound, "call_invite missing call_id");
                                return Ok(());
                            }
                            if !invite.media.audio_enabled && !invite.media.video_enabled {
                                emit_error(outbound, "call_invite must enable audio or video");
                                return Ok(());
                            }
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            invite.from_user_id = from_id;
                            match webrtc::process_call_invite(Arc::clone(&state), invite).await {
                                Ok(()) => {}
                                Err(err) => {
                                    emit_error(outbound, &format!("call_invite rejected: {err}"));
                                }
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid call_invite format");
                        }
                    }
                }
                "call_answer" => {
                    match serde_json::from_str::<rura_models::webrtc::CallAnswer>(&msg.data) {
                        Ok(mut answer) => {
                            if answer.call_id.trim().is_empty() {
                                emit_error(outbound, "call_answer missing call_id");
                                return Ok(());
                            }
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            answer.from_user_id = from_id;
                            match webrtc::process_call_answer(Arc::clone(&state), answer).await {
                                Ok(()) => {}
                                Err(err) => {
                                    emit_error(outbound, &format!("call_answer rejected: {err}"));
                                }
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid call_answer format");
                        }
                    }
                }
                "call_reject" => {
                    match serde_json::from_str::<rura_models::webrtc::CallReject>(&msg.data) {
                        Ok(mut reject) => {
                            if reject.call_id.trim().is_empty() {
                                emit_error(outbound, "call_reject missing call_id");
                                return Ok(());
                            }
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            reject.from_user_id = from_id;
                            match webrtc::process_call_reject(Arc::clone(&state), reject).await {
                                Ok(()) => {}
                                Err(err) => {
                                    emit_error(outbound, &format!("call_reject rejected: {err}"));
                                }
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid call_reject format");
                        }
                    }
                }
                "call_hangup" => {
                    match serde_json::from_str::<rura_models::webrtc::CallHangup>(&msg.data) {
                        Ok(mut hangup) => {
                            if hangup.call_id.trim().is_empty() {
                                emit_error(outbound, "call_hangup missing call_id");
                                return Ok(());
                            }
                            let Some(from_id) = require_session_id(session_user_id, outbound)
                            else {
                                return Ok(());
                            };
                            hangup.from_user_id = from_id;
                            match webrtc::process_call_hangup(Arc::clone(&state), hangup).await {
                                Ok(()) => {}
                                Err(err) => {
                                    emit_error(outbound, &format!("call_hangup rejected: {err}"));
                                }
                            }
                        }
                        Err(_) => {
                            emit_error(outbound, "Invalid call_hangup format");
                        }
                    }
                }
                "history" => {
                    // Server does not persist messages; return empty history.
                    // Still respond with a proper history_response envelope for clients expecting it.
                    // Ignore request contents (limit) since server does not persist messages.
                    #[derive(serde::Serialize)]
                    struct HistResp<'a> {
                        success: bool,
                        message: &'a str,
                        messages: Vec<crate::messaging::models::HistoryMessage>,
                    }
                    // Return an empty list; clients may merge with local cache.
                    let resp = HistResp {
                        success: true,
                        message: "OK",
                        messages: Vec::new(),
                    };
                    let wrapper = ClientMessage {
                        command: "history_response".to_string(),
                        data: serde_json::to_string(&resp).unwrap(),
                    };
                    let _ = outbound.send(wrapper);
                }
                "set_pubkey" => {
                    #[derive(serde::Deserialize)]
                    struct SetPkReq {
                        pubkey: String,
                    }
                    #[derive(serde::Serialize)]
                    struct SetPkResp {
                        success: bool,
                        message: String,
                    }
                    match serde_json::from_str::<SetPkReq>(&msg.data) {
                        Ok(req) => {
                            // TEMPORARY: print identity + pubkey for manual sharing
                            println!("(TEMPORARY) User {} set PubKey: {}", user_id, req.pubkey);
                            // Attempt DB update only if user_id parses as numeric (legacy flow)
                            let result = if let Ok(uid) = user_id.parse::<i64>() {
                                set_user_pubkey(Arc::clone(&conn), uid, &req.pubkey).await
                            } else {
                                Ok(true)
                            };
                            match result {
                                Ok(true) => {
                                    let resp = SetPkResp {
                                        success: true,
                                        message: "Pubkey stored".to_string(),
                                    };
                                    let wrapper = ClientMessage {
                                        command: "set_pubkey_response".to_string(),
                                        data: serde_json::to_string(&resp).unwrap(),
                                    };
                                    let _ = outbound.send(wrapper);
                                }
                                Ok(false) => {
                                    let resp = SetPkResp {
                                        success: false,
                                        message: "User not found".to_string(),
                                    };
                                    let wrapper = ClientMessage {
                                        command: "set_pubkey_response".to_string(),
                                        data: serde_json::to_string(&resp).unwrap(),
                                    };
                                    let _ = outbound.send(wrapper);
                                }
                                Err(_) => {
                                    let resp = SetPkResp {
                                        success: false,
                                        message: "Failed to store pubkey".to_string(),
                                    };
                                    let wrapper = ClientMessage {
                                        command: "set_pubkey_response".to_string(),
                                        data: serde_json::to_string(&resp).unwrap(),
                                    };
                                    let _ = outbound.send(wrapper);
                                }
                            }
                        }
                        Err(_) => {
                            let err = ClientMessage {
                                command: "error".to_string(),
                                data: "Invalid set_pubkey format".to_string(),
                            };
                            let _ = outbound.send(err);
                        }
                    }
                }
                "get_pubkey" => {
                    #[derive(serde::Deserialize)]
                    struct GetPkReq {
                        user_id: i64,
                    }
                    #[derive(serde::Serialize)]
                    struct GetPkResp {
                        success: bool,
                        message: String,
                        user_id: Option<i64>,
                        pubkey: Option<String>,
                    }
                    match serde_json::from_str::<GetPkReq>(&msg.data) {
                        Ok(req) => match get_user_pubkey(Arc::clone(&conn), req.user_id).await {
                            Ok(Some(pk)) => {
                                let resp = GetPkResp {
                                    success: true,
                                    message: "OK".to_string(),
                                    user_id: Some(req.user_id),
                                    pubkey: Some(pk),
                                };
                                let wrapper = ClientMessage {
                                    command: "get_pubkey_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Ok(None) => {
                                let resp = GetPkResp {
                                    success: false,
                                    message: "User not found or no pubkey".to_string(),
                                    user_id: Some(req.user_id),
                                    pubkey: None,
                                };
                                let wrapper = ClientMessage {
                                    command: "get_pubkey_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Err(_) => {
                                let resp = GetPkResp {
                                    success: false,
                                    message: "Failed to load pubkey".to_string(),
                                    user_id: Some(req.user_id),
                                    pubkey: None,
                                };
                                let wrapper = ClientMessage {
                                    command: "get_pubkey_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                        },
                        Err(_) => {
                            let err = ClientMessage {
                                command: "error".to_string(),
                                data: "Invalid get_pubkey format".to_string(),
                            };
                            let _ = outbound.send(err);
                        }
                    }
                }
                // default: echo back via outbound to keep behavior simple
                _ => {
                    let _ = outbound.send(msg);
                }
            }
            Ok(())
        }
        Err(_) => {
            // Parsing failed; notify sender via outbound
            let err = ClientMessage {
                command: "error".to_string(),
                data: "Invalid JSON".to_string(),
            };
            let _ = outbound.send(err);
            Ok(())
        }
    }
}
