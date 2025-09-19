use tokio::io::AsyncWriteExt;
use crate::models::client_message::{ClientMessage, AuthResponse};

pub async fn send_auth_success_response(
    stream: &mut tokio::net::TcpStream,
    user_id: i64,
    message: &str
) -> tokio::io::Result<()> {
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

pub async fn send_auth_error_response(
    stream: &mut tokio::net::TcpStream,
    message: &str
) -> tokio::io::Result<()> {
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
