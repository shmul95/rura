use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::models::client_message::ClientMessage;

use super::{authed, unauth};
use crate::messaging::state::AppState;

pub(super) async fn handle_read_success<S>(
    stream: &mut S,
    conn: Arc<Mutex<Connection>>,
    state: Arc<AppState>,
    client_addr: SocketAddr,
    authenticated_user_id: &mut Option<i64>,
    outbound_tx: Option<&mpsc::UnboundedSender<ClientMessage>>,
    buffer: &[u8],
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    if let Some(user_id) = *authenticated_user_id {
        // User is authenticated, allow normal communication
        let tx = outbound_tx.expect("outbound sender not set for authenticated user");
        authed::handle_client_message(
            Arc::clone(&state),
            Arc::clone(&conn),
            tx,
            client_addr,
            user_id,
            buffer,
        )
        .await
    } else {
        // User not authenticated, only allow auth commands
        let received = String::from_utf8_lossy(buffer).to_string();
        match serde_json::from_str::<ClientMessage>(&received) {
            Ok(msg) => {
                unauth::handle_unauthenticated_message(
                    stream,
                    Arc::clone(&conn),
                    client_addr,
                    msg,
                    authenticated_user_id,
                )
                .await
            }
            Err(e) => unauth::handle_unauthenticated_parse_error(stream, client_addr, e).await,
        }
    }
}
