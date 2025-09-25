use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::select;
use tokio::sync::mpsc;

use crate::messaging::state::{AppState, ClientHandle};

use super::{dispatch, io_helpers};

pub(super) async fn handle_client_loop(
    stream: &mut tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
    state: Arc<AppState>,
    client_addr: SocketAddr,
) -> tokio::io::Result<()> {
    let mut buffer = [0; 1024];
    let mut authenticated_user_id: Option<i64> = None;
    let mut outbound_tx: Option<
        mpsc::UnboundedSender<crate::models::client_message::ClientMessage>,
    > = None;
    let mut outbound_rx: Option<
        mpsc::UnboundedReceiver<crate::models::client_message::ClientMessage>,
    > = None;

    loop {
        if let Some(rx) = outbound_rx.as_mut() {
            select! {
                read_res = stream.read(&mut buffer) => {
                    match read_res {
                        Ok(0) => {
                            io_helpers::handle_connection_closed(client_addr).await;
                            break;
                        }
                        Ok(n) => {
                            let was_unauth = authenticated_user_id.is_none();
                            dispatch::handle_read_success(
                                stream,
                                Arc::clone(&conn),
                                Arc::clone(&state),
                                client_addr,
                                &mut authenticated_user_id,
                                outbound_tx.as_ref(),
                                &buffer[..n],
                            ).await?;

                            if was_unauth {
                                if let Some(user_id) = authenticated_user_id {
                                    let (tx, new_rx) = mpsc::unbounded_channel();
                                    state.register(user_id, ClientHandle { tx: tx.clone() }).await;
                                    outbound_tx = Some(tx);
                                    outbound_rx = Some(new_rx);
                                }
                            }
                        }
                        Err(e) => {
                            io_helpers::handle_read_error(client_addr, e).await;
                            break;
                        }
                    }
                },
                maybe_msg = rx.recv() => {
                    match maybe_msg {
                        Some(msg) => {
                            if let Ok(mut json) = serde_json::to_string(&msg) {
                                json.push('\n');
                                if let Err(e) = stream.write_all(json.as_bytes()).await {
                                    io_helpers::handle_read_error(client_addr, e).await;
                                    break;
                                }
                                let _ = stream.flush().await;
                            }
                        }
                        None => {
                            // Sender dropped; keep loop running and wait for read events
                        }
                    }
                }
            }
        } else {
            match stream.read(&mut buffer).await {
                Ok(0) => {
                    io_helpers::handle_connection_closed(client_addr).await;
                    break;
                }
                Ok(n) => {
                    let was_unauth = authenticated_user_id.is_none();
                    dispatch::handle_read_success(
                        stream,
                        Arc::clone(&conn),
                        Arc::clone(&state),
                        client_addr,
                        &mut authenticated_user_id,
                        outbound_tx.as_ref(),
                        &buffer[..n],
                    )
                    .await?;

                    // If we just became authenticated, set up outbound channel and register
                    if was_unauth {
                        if let Some(user_id) = authenticated_user_id {
                            let (tx, rx) = mpsc::unbounded_channel();
                            state
                                .register(user_id, ClientHandle { tx: tx.clone() })
                                .await;
                            outbound_tx = Some(tx);
                            outbound_rx = Some(rx);
                        }
                    }
                }
                Err(e) => {
                    io_helpers::handle_read_error(client_addr, e).await;
                    break;
                }
            }
        }
    }
    // Cleanup: unregister user and drop outbound sender if any
    if let Some(user_id) = authenticated_user_id {
        state.unregister(user_id).await;
    }
    Ok(())
}
