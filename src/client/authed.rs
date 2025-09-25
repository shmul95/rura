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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tokio::time::{timeout, Duration};

    fn test_addr() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080)
    }

    #[tokio::test]
    async fn test_invalid_message_format_sends_error_to_sender() {
        let state = Arc::new(AppState::default());
        let (tx_out, mut rx_out) = mpsc::unbounded_channel::<ClientMessage>();

        // Build a ClientMessage with command "message" but invalid JSON in data
        let wire = ClientMessage {
            command: "message".to_string(),
            data: "not json".to_string(),
        };
        let wire_str = serde_json::to_string(&wire).unwrap();

        // Call the handler as if it received this line
        handle_client_message(
            Arc::clone(&state),
            &tx_out,
            test_addr(),
            1,
            wire_str.as_bytes(),
        )
        .await
        .unwrap();

        // Expect an error response to be queued back to the sender via outbound
        let resp = timeout(Duration::from_millis(100), rx_out.recv())
            .await
            .expect("timed out waiting for outbound")
            .expect("outbound channel closed");

        assert_eq!(resp.command, "error");
        assert_eq!(resp.data, "Invalid message format");
    }
}
