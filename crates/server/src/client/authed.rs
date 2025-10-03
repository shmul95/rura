use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::messaging::handlers::send_direct;
use crate::messaging::models::{
    DirectMessageReq, HistoryRequest, HistoryResponse, SaveRequest, SaveResponse,
};
use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use crate::utils::db_utils::set_message_saved;
use rusqlite::Connection;

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
                    match serde_json::from_str::<DirectMessageReq>(&msg.data) {
                        Ok(req) => {
                            send_direct(Arc::clone(&state), Arc::clone(&conn), user_id, req)
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
                "history" => match serde_json::from_str::<HistoryRequest>(&msg.data) {
                    Ok(req) => {
                        let limit = req.limit.unwrap_or(100);
                        match crate::utils::db_utils::fetch_messages_for_user(
                            Arc::clone(&conn),
                            user_id,
                            limit,
                        )
                        .await
                        {
                            Ok(messages) => {
                                let resp = HistoryResponse {
                                    success: true,
                                    message: "OK".to_string(),
                                    messages,
                                };
                                let wrapper = ClientMessage {
                                    command: "history_response".to_string(),
                                    data: serde_json::to_string(&resp).unwrap(),
                                };
                                let _ = outbound.send(wrapper);
                            }
                            Err(_) => {
                                let resp = HistoryResponse {
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
                "save" => match serde_json::from_str::<SaveRequest>(&msg.data) {
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
                                let resp = SaveResponse {
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
                                let resp = SaveResponse {
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
                                let resp = SaveResponse {
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
