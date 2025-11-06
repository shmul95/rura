use rura_server::messaging::handlers::send_direct;
use rura_server::messaging::models::DirectMessageReq;
use rura_server::messaging::state::{AppState, ClientHandle};
use rura_server::models::client_message::ClientMessage;
use rura_server::webrtc;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn direct_message_bypasses_tcp_when_webrtc_active() {
    let state = Arc::new(AppState::default());
    let conn = Arc::new(Mutex::new(Connection::open(":memory:").unwrap()));

    // Register two users
    let (tx_alice, mut rx_alice) = mpsc::unbounded_channel::<ClientMessage>();
    let (tx_bob, mut rx_bob) = mpsc::unbounded_channel::<ClientMessage>();
    state
        .register(1.to_string(), ClientHandle { tx: tx_alice })
        .await;
    state
        .register(2.to_string(), ClientHandle { tx: tx_bob })
        .await;

    // Mark a session active by handling an offer
    let offer = rura_models::webrtc::RtcOffer {
        from_user_id: 1,
        to_user_id: 2,
        sdp: "O".into(),
    };
    webrtc::process_offer(Arc::clone(&state), offer).await;
    // Drain the forwarded offer to Bob (not essential)
    let _ = timeout(Duration::from_millis(50), rx_bob.recv()).await;

    // Attempt to send a direct message via server relay; should be bypassed.
    let req = DirectMessageReq {
        to_user_id: 2,
        body: "hello over rtc".into(),
    };
    send_direct(Arc::clone(&state), Arc::clone(&conn), 1, req)
        .await
        .expect("send_direct should not error");

    // Assert Bob did not get a TCP relayed 'message'
    let res = timeout(Duration::from_millis(50), rx_bob.recv()).await;
    assert!(
        res.is_err(),
        "no TCP message should be delivered when RTC active"
    );

    // Alice should also not get anything
    let res2 = timeout(Duration::from_millis(50), rx_alice.recv()).await;
    assert!(res2.is_err());
}
