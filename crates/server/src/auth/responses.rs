use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::models::client_message::{AuthResponse, ClientMessage};

pub async fn send_auth_success_response<W>(
    stream: &mut W,
    user_id: i64,
    message: &str,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let response = ClientMessage {
        command: "auth_response".to_string(),
        data: serde_json::to_string(&AuthResponse {
            success: true,
            message: message.to_string(),
            user_id: Some(user_id),
        })
        .unwrap(),
    };
    let response_str = serde_json::to_string(&response)? + "\n";
    stream.write_all(response_str.as_bytes()).await?;
    stream.flush().await
}

pub async fn send_auth_error_response<W>(stream: &mut W, message: &str) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let response = ClientMessage {
        command: "auth_response".to_string(),
        data: serde_json::to_string(&AuthResponse {
            success: false,
            message: message.to_string(),
            user_id: None,
        })
        .unwrap(),
    };
    let response_str = serde_json::to_string(&response)? + "\n";
    stream.write_all(response_str.as_bytes()).await?;
    stream.flush().await
}
