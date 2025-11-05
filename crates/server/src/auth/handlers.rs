use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::responses::{
    send_auth_error_response, send_auth_success_response, send_auth_success_response_identity,
};
use crate::models::client_message::{AuthRequest, ClientMessage};
use crate::utils::db_utils::{authenticate_user, register_user};

pub async fn handle_auth_command_error<W>(stream: &mut W) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    let error_msg = ClientMessage {
        command: "error".to_string(),
        data: "Authentication required. Please send 'login' or 'register' command first"
            .to_string(),
    };
    let response = serde_json::to_string(&error_msg)? + "\n";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(None)
}

pub async fn handle_auth_success<W>(
    stream: &mut W,
    client_addr: SocketAddr,
    identity_key: String,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    // Identity-based auth: return id string (no numeric user_id)
    send_auth_success_response_identity(stream, &identity_key, "Authentication successful").await?;
    println!(
        "User {} authenticated successfully from {}",
        identity_key, client_addr
    );
    // TEMPORARY: Print identity on auth so it can be shared
    println!("(TEMPORARY) Client ID: {}", identity_key);
    Ok(Some(identity_key))
}

pub async fn handle_auth_failure<W>(stream: &mut W) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    send_auth_error_response(stream, "Invalid passphrase or password").await?;
    Ok(None)
}

pub async fn handle_auth_db_error<W>(
    stream: &mut W,
    e: rusqlite::Error,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    eprintln!("Database error during authentication: {}", e);
    send_auth_error_response(stream, "Authentication error").await?;
    Ok(None)
}

pub async fn handle_auth_parse_error<W>(
    stream: &mut W,
    client_addr: SocketAddr,
    e: serde_json::Error,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    eprintln!("Invalid auth data from {}: {}", client_addr, e);
    send_auth_error_response(stream, "Invalid authentication format").await?;
    Ok(None)
}

pub async fn handle_registration_success<W>(
    stream: &mut W,
    client_addr: SocketAddr,
    identity_key: String,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    // Identity-based registration: return id string
    send_auth_success_response_identity(stream, &identity_key, "Registration successful").await?;
    println!(
        "User {} registered successfully from {}",
        identity_key, client_addr
    );
    // TEMPORARY: Print identity on registration so it can be shared
    println!("(TEMPORARY) Client ID: {}", identity_key);
    Ok(Some(identity_key))
}

pub async fn handle_registration_error<W>(
    stream: &mut W,
    e: rusqlite::Error,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    eprintln!("Registration error: {}", e);
    let message = if e.to_string().contains("already exists") {
        "User with this passphrase already exists"
    } else {
        "Registration failed"
    };
    send_auth_error_response(stream, message).await?;
    Ok(None)
}

pub async fn handle_registration_parse_error<W>(
    stream: &mut W,
    client_addr: SocketAddr,
    e: serde_json::Error,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    eprintln!("Invalid registration data from {}: {}", client_addr, e);
    send_auth_error_response(stream, "Invalid registration format").await?;
    Ok(None)
}

pub async fn handle_auth_login<W>(
    stream: &mut W,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
    msg: &ClientMessage,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    match serde_json::from_str::<serde_json::Value>(&msg.data) {
        Ok(val) => {
            // Accept either {"id": "..."} or {"identity_key": "..."}
            let id_field = val
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    val.get("identity_key")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            if let Some(identity_key) = id_field {
                return handle_auth_success(stream, client_addr, identity_key).await;
            }
            // Fallback to legacy DB auth if provided
            match serde_json::from_value::<AuthRequest>(val) {
                Ok(login_data) => match authenticate_user(
                    Arc::clone(&conn),
                    &login_data.passphrase,
                    &login_data.password,
                )
                .await
                {
                    Ok(Some(user_id)) => {
                        handle_auth_success(stream, client_addr, user_id.to_string()).await
                    }
                    Ok(None) => handle_auth_failure(stream).await,
                    Err(e) => handle_auth_db_error(stream, e).await,
                },
                Err(e) => handle_auth_parse_error(stream, client_addr, e).await,
            }
        }
        Err(e) => handle_auth_parse_error(stream, client_addr, e).await,
    }
}

pub async fn handle_auth_register<W>(
    stream: &mut W,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
    msg: &ClientMessage,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    match serde_json::from_str::<serde_json::Value>(&msg.data) {
        Ok(val) => {
            let id_field = val
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    val.get("identity_key")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            if let Some(identity_key) = id_field {
                return handle_registration_success(stream, client_addr, identity_key).await;
            }
            // Fallback to legacy DB registration if provided
            match serde_json::from_value::<AuthRequest>(val) {
                Ok(register_data) => match register_user(
                    Arc::clone(&conn),
                    &register_data.passphrase,
                    &register_data.password,
                )
                .await
                {
                    Ok(user_id) => {
                        handle_registration_success(stream, client_addr, user_id.to_string()).await
                    }
                    Err(e) => handle_registration_error(stream, e).await,
                },
                Err(e) => handle_registration_parse_error(stream, client_addr, e).await,
            }
        }
        Err(e) => handle_registration_parse_error(stream, client_addr, e).await,
    }
}

pub async fn handle_auth<W>(
    stream: &mut W,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
    message: &ClientMessage,
) -> tokio::io::Result<Option<String>>
where
    W: AsyncWrite + Unpin,
{
    match message.command.as_str() {
        "login" => handle_auth_login(stream, conn, client_addr, message).await,
        "register" => handle_auth_register(stream, conn, client_addr, message).await,
        _ => handle_auth_command_error(stream).await,
    }
}
