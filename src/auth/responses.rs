use crate::models::client_message::{AuthResponse, ClientMessage};
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub async fn send_auth_success_response<W>(
    stream: &mut W,
    user_id: i64,
    message: &str,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let response = AuthResponse {
        success: true,
        message: message.to_string(),
        user_id: Some(user_id),
    };
    let response_msg = ClientMessage {
        command: "auth_response".to_string(),
        data: serde_json::to_string(&response)?,
    };
    let response_json = serde_json::to_string(&response_msg)? + "\n";
    stream.write_all(response_json.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn send_auth_error_response<W>(stream: &mut W, message: &str) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let response = AuthResponse {
        success: false,
        message: message.to_string(),
        user_id: None,
    };
    let response_msg = ClientMessage {
        command: "auth_response".to_string(),
        data: serde_json::to_string(&response)?,
    };
    let response_json = serde_json::to_string(&response_msg)? + "\n";
    stream.write_all(response_json.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}
