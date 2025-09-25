use rura::messaging::state::{AppState, ClientHandle};
use rura::models::client_message::ClientMessage;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_register_get_unregister_sender() {
    let state = Arc::new(AppState::default());
    let (tx, mut rx) = mpsc::unbounded_channel::<ClientMessage>();

    // No sender registered yet
    assert!(state.get_sender(1).await.is_none());

    // Register
    state.register(1, ClientHandle { tx: tx.clone() }).await;

    // Get and send a message
    let got_tx = state.get_sender(1).await.expect("expected sender");
    got_tx
        .send(ClientMessage {
            command: "ping".into(),
            data: "pong".into(),
        })
        .unwrap();

    // Verify receiver gets it
    let msg = rx.recv().await.expect("channel should have a message");
    assert_eq!(msg.command, "ping");
    assert_eq!(msg.data, "pong");

    // Unregister
    state.unregister(1).await;
    assert!(state.get_sender(1).await.is_none());
}
