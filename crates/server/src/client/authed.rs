use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::messaging::handlers::send_direct;
use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::{fetch_messages_for_user, set_message_saved};
use rusqlite::Connection;

#[derive(serde::Deserialize)]
struct LocalHistoryRequest {
    limit: Option<usize>,
}

#[derive(serde::Serialize)]
struct LocalHistoryMessage {
    id: i64,
    from_user_id: i64,
    to_user_id: i64,
    body: String,
    timestamp: String,
    saved: bool,
}

#[derive(serde::Serialize)]
struct LocalHistoryResponse {
    success: bool,
    message: String,
    messages: Vec<LocalHistoryMessage>,
}

#[derive(serde::Deserialize)]
struct LocalSaveRequest {
    message_id: i64,
    saved: Option<bool>,
}

#[derive(serde::Serialize)]
struct LocalSaveResponse {
    success: bool,
    message: String,
    message_id: Option<i64>,
    saved: Option<bool>,
}

pub(super) async fn handle_client_message(
    state: Arc<AppState>,
    conn: Arc<Mutex<Connection>>,
    outbound: &mpsc::UnboundedSender<ClientMessage>,
    client_addr: SocketAddr,
    user_id: i64,
    buffer: &[u8],
) -> tokio::io::Result<()> {
    let received = String::from_utf8_lossy(buffer).to_string();
    match serde_json::from_str::<ClientMessage>(&received) {
        Ok(msg) => {
            println!(
                "Received from authenticated user {} ({}): {:?}",
                user_id, client_addr, msg
            );
            match msg.command.as_str() {
                "message" => {
                    #[derive(serde::Deserialize)]
                    struct LocalDM {
                        to_user_id: i64,
                        body: String,
                        saved: Option<bool>,
                    }
                    match serde_json::from_str::<LocalDM>(&msg.data) {
                        Ok(req) => {
                            let req2 = crate::messaging::models::DirectMessageReq {
                                to_user_id: req.to_user_id,
                                body: req.body,
                                saved: req.saved,
                            };
                            send_direct(Arc::clone(&state), Arc::clone(&conn), user_id, req2)
                                .await?;
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
                "history" => match serde_json::from_str::<LocalHistoryRequest>(&msg.data) {
                    Ok(req) => {
                        let limit = req.limit.unwrap_or(100);
                        match fetch_messages_for_user(Arc::clone(&conn), user_id, limit).await {
                            Ok(messages) => {
                                let mapped: Vec<LocalHistoryMessage> = messages
                                    .into_iter()
                                    .map(|m| LocalHistoryMessage {
                                        id: m.id,
                                        from_user_id: m.sender,
                                        to_user_id: m.receiver,
                                        body: m.content,
                                        timestamp: m.timestamp,
                                        saved: m.saved,
                                    })
                                    .collect();
                                let resp = LocalHistoryResponse {
                                    success: true,
                                    message: "OK".to_string(),
                                    messages: mapped,
                                };
                                let wrapper = ClientMessage {
                                    command: "history_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Err(_) => {
                                let resp = LocalHistoryResponse {
                                    success: false,
                                    message: "Failed to load history".to_string(),
                                    messages: Vec::new(),
                                };
                                let wrapper = ClientMessage {
                                    command: "history_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                        }
                    }
                    Err(_) => {
                        let err = ClientMessage {
                            command: "error".to_string(),
                            data: "Invalid history format".to_string(),
                        };
                        let _ = outbound.send(err);
                    }
                },
                "save" => match serde_json::from_str::<LocalSaveRequest>(&msg.data) {
                    Ok(req) => {
                        let saved_flag = req.saved.unwrap_or(true);
                        match set_message_saved(
                            Arc::clone(&conn),
                            user_id,
                            req.message_id,
                            saved_flag,
                        )
                        .await
                        {
                            Ok(true) => {
                                let resp = LocalSaveResponse {
                                    success: true,
                                    message: "Message updated".to_string(),
                                    message_id: Some(req.message_id),
                                    saved: Some(saved_flag),
                                };
                                let wrapper = ClientMessage {
                                    command: "save_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Ok(false) => {
                                let resp = LocalSaveResponse {
                                    success: false,
                                    message: "Message not found or not authorized".to_string(),
                                    message_id: Some(req.message_id),
                                    saved: Some(saved_flag),
                                };
                                let wrapper = ClientMessage {
                                    command: "save_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Err(_) => {
                                let resp = LocalSaveResponse {
                                    success: false,
                                    message: "Failed to update message".to_string(),
                                    message_id: Some(req.message_id),
                                    saved: Some(saved_flag),
                                };
                                let wrapper = ClientMessage {
                                    command: "save_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                        }
                    }
                    Err(_) => {
                        let err = ClientMessage {
                            command: "error".to_string(),
                            data: "Invalid save format".to_string(),
                        };
                        let _ = outbound.send(err);
                    }
                },
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
