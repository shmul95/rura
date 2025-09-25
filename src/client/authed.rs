use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::messaging::handlers::send_direct;
use crate::messaging::models::DirectMessageReq;
use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;

pub(super) async fn handle_client_message(
    state: Arc<AppState>,
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
                            send_direct(Arc::clone(&state), user_id, req).await?;
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
                // default: echo back via outbound to keep behavior simple
                _ => {
                    let _ = outbound.send(msg);
                }
            }
            Ok(())
        }
        Err(_) => {
            // Parsing failed; notify sender via outbound
            let error_msg = ClientMessage {
                command: "error".to_string(),
                data: "Invalid JSON".to_string(),
            };
            let _ = outbound.send(error_msg);
            Ok(())
        }
    }
}
