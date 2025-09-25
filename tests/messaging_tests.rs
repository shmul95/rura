use rura::messaging::handlers::send_direct;
use rura::messaging::models::{DirectMessageEvent, DirectMessageReq};
use rura::messaging::state::{AppState, ClientHandle};
use rura::models::client_message::ClientMessage;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn test_send_direct_to_online_user_delivers_message() {
    let state = Arc::new(AppState::default());

    // Simulate recipient user with an outbound channel registered in state
    let (tx_bob, mut rx_bob) = mpsc::unbounded_channel::<ClientMessage>();
    let bob_id = 2_i64;
    state.register(bob_id, ClientHandle { tx: tx_bob }).await;

    // Sender user id
    let alice_id = 1_i64;

    // Send a direct message to Bob
    let req = DirectMessageReq {
        to_user_id: bob_id,
        body: "hello world".to_string(),
    };
    send_direct(Arc::clone(&state), alice_id, req)
        .await
        .unwrap();

    // Bob should receive a ClientMessage with command "message"
    let delivered: ClientMessage = timeout(Duration::from_millis(100), rx_bob.recv())
        .await
        .expect("timed out waiting for message")
        .expect("channel closed unexpectedly");

    assert_eq!(delivered.command, "message");

    // The data should parse into DirectMessageEvent with the correct sender and body
    let event: DirectMessageEvent = serde_json::from_str(&delivered.data).unwrap();
    assert_eq!(event.from_user_id, alice_id);
    assert_eq!(event.body, "hello world");
}

#[tokio::test]
async fn test_send_direct_to_unknown_user_sends_nothing() {
    let state = Arc::new(AppState::default());

    // Create a channel for some other user and register them (not the target)
    let (tx_other, mut rx_other) = mpsc::unbounded_channel::<ClientMessage>();
    state.register(999, ClientHandle { tx: tx_other }).await;

    // Attempt to send to a user id that is not registered
    let unknown_user_id = 12345_i64;
    let from_user_id = 1_i64;
    let req = DirectMessageReq {
        to_user_id: unknown_user_id,
        body: "are you there?".to_string(),
    };
    send_direct(Arc::clone(&state), from_user_id, req)
        .await
        .expect("send_direct should not error for unknown user");

    // Ensure no message was delivered to the registered 'other' user
    let res = timeout(Duration::from_millis(50), rx_other.recv()).await;
    assert!(res.is_err(), "no message should be delivered to others");
}
