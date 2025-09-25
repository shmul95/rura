use rusqlite::Connection;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

use super::{dispatch, io_helpers};

pub(super) async fn handle_client_loop(
    stream: &mut tokio::net::TcpStream,
    conn: Arc<Mutex<Connection>>,
    client_addr: SocketAddr,
) -> tokio::io::Result<()> {
    let mut buffer = [0; 1024];
    let mut authenticated_user_id: Option<i64> = None;

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                io_helpers::handle_connection_closed(client_addr).await;
                break;
            }
            Ok(n) => {
                dispatch::handle_read_success(
                    stream,
                    Arc::clone(&conn),
                    client_addr,
                    &mut authenticated_user_id,
                    &buffer[..n],
                )
                .await?;
            }
            Err(e) => {
                io_helpers::handle_read_error(client_addr, e).await;
                break;
            }
        }
    }
    Ok(())
}
