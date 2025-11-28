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
async fn direct_message_relays_even_when_rtc_active() {
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

    // Mark a session active by going through call invite/answer flow and offer
    let invite = rura_models::webrtc::CallInvite {
        call_id: "call-789".to_string(),
        from_user_id: 1,
        to_user_id: 2,
        media: rura_models::webrtc::CallMediaProfile {
            audio_enabled: true,
            video_enabled: false,
            audio_muted: Some(false),
            video_muted: Some(true),
        },
        preview: None,
        client: None,
        ringing_timeout_ms: None,
    };
    webrtc::process_call_invite(Arc::clone(&state), invite.clone())
        .await
        .expect("invite ok");
    let answer = rura_models::webrtc::CallAnswer {
        call_id: invite.call_id.clone(),
        from_user_id: 2,
        to_user_id: 1,
        resume_media: None,
    };
    webrtc::process_call_answer(Arc::clone(&state), answer)
        .await
        .expect("answer ok");
    let offer = rura_models::webrtc::RtcOffer {
        from_user_id: 1,
        to_user_id: 2,
        sdp: "O".into(),
        call_id: Some(invite.call_id.clone()),
    };
    webrtc::process_offer(Arc::clone(&state), offer)
        .await
        .expect("offer ok");
    // Drain signaling events (invite, ringing, offer for Bob; ringing + answer for Alice)
    for _ in 0..3 {
        let _ = timeout(Duration::from_millis(50), rx_bob.recv()).await;
    }
    for _ in 0..2 {
        let _ = timeout(Duration::from_millis(50), rx_alice.recv()).await;
    }

    // Attempt to send a direct message via server relay; should still deliver.
    let req = DirectMessageReq {
        to_user_id: 2,
        body: "hello over rtc".into(),
    };
    send_direct(Arc::clone(&state), Arc::clone(&conn), 1, req)
        .await
        .expect("send_direct should not error");

    // Assert Bob receives the TCP relayed 'message'
    let delivered = timeout(Duration::from_millis(100), rx_bob.recv())
        .await
        .expect("delivery timeout")
        .expect("channel closed");
    assert_eq!(delivered.command, "message");
    let payload: rura_models::messaging::DirectMessageEvent =
        serde_json::from_str(&delivered.data).expect("payload");
    assert_eq!(payload.from_user_id, 1);
    assert_eq!(payload.body, "hello over rtc");

    // Alice should not receive a 'message' command since relay only targets recipient
    let res2 = timeout(Duration::from_millis(50), rx_alice.recv()).await;
    assert!(res2.is_err());
}
