use rura_server::messaging::handlers::send_direct;
use rura_server::messaging::models::DirectMessageReq;
use rura_server::messaging::state::{AppState, ClientHandle};
use rura_server::models::client_message::ClientMessage;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn direct_message_is_not_relayed_over_tcp() {
    let state = Arc::new(AppState::default());
    let conn = Arc::new(Mutex::new(Connection::open(":memory:").unwrap()));
    {
        let c = conn.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender INTEGER NOT NULL,
                receiver INTEGER NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                saved INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
    }

    // Simulate recipient user with an outbound channel registered in state
    let (tx_bob, mut rx_bob) = mpsc::unbounded_channel::<ClientMessage>();
    let bob_id = 2_i64;
    state
        .register(bob_id.to_string(), ClientHandle { tx: tx_bob })
        .await;

    // Sender user id
    let alice_id = 1_i64;

    // Attempt to send a direct message to Bob (server should not relay bodies)
    let req = DirectMessageReq {
        to_user_id: bob_id,
        body: "hello world".to_string(),
    };
    send_direct(Arc::clone(&state), Arc::clone(&conn), alice_id, req)
        .await
        .unwrap();

    // Bob should NOT receive a relayed message over TCP
    let res = timeout(Duration::from_millis(100), rx_bob.recv()).await;
    assert!(
        res.is_err(),
        "server must not relay message bodies over TCP"
    );
}

#[tokio::test]
async fn test_send_direct_to_unknown_user_sends_nothing() {
    let state = Arc::new(AppState::default());
    let conn = Arc::new(Mutex::new(Connection::open(":memory:").unwrap()));
    {
        let c = conn.lock().unwrap();
        c.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sender INTEGER NOT NULL,
                receiver INTEGER NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                saved INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .unwrap();
    }

    // Create a channel for some other user and register them (not the target)
    let (tx_other, mut rx_other) = mpsc::unbounded_channel::<ClientMessage>();
    state
        .register(999.to_string(), ClientHandle { tx: tx_other })
        .await;

    // Attempt to send to a user id that is not registered
    let unknown_user_id = 12345_i64;
    let from_user_id = 1_i64;
    let req = DirectMessageReq {
        to_user_id: unknown_user_id,
        body: "are you there?".to_string(),
    };
    send_direct(Arc::clone(&state), Arc::clone(&conn), from_user_id, req)
        .await
        .expect("send_direct should not error for unknown user");

    // Ensure no message was delivered to the registered 'other' user
    let res = timeout(Duration::from_millis(50), rx_other.recv()).await;
    assert!(res.is_err(), "no message should be delivered to others");

    // Server no longer persists messages; ensure nothing was stored
    let count: i64 = {
        let c = conn.lock().unwrap();
        c.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(count, 0);
}
