use rura_server::messaging::state::{AppState, ClientHandle};
use rura_server::models::client_message::ClientMessage;
use rura_server::webrtc;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn webrtc_offer_answer_forwarding() {
    let state = Arc::new(AppState::default());

    // Register Alice (1) and Bob (2) with outbound channels
    let (tx_alice, mut rx_alice) = mpsc::unbounded_channel::<ClientMessage>();
    let (tx_bob, mut rx_bob) = mpsc::unbounded_channel::<ClientMessage>();
    state
        .register(1.to_string(), ClientHandle { tx: tx_alice })
        .await;
    state
        .register(2.to_string(), ClientHandle { tx: tx_bob })
        .await;

    // Alice -> Bob: offer should be forwarded to Bob
    let offer = rura_models::webrtc::RtcOffer {
        from_user_id: 1,
        to_user_id: 2,
        sdp: "O".to_string(),
    };
    webrtc::process_offer(Arc::clone(&state), offer.clone()).await;
    let delivered_bob = timeout(Duration::from_millis(100), rx_bob.recv())
        .await
        .expect("offer timeout")
        .expect("offer channel closed");
    assert_eq!(delivered_bob.command, "rtc_offer");
    let parsed: rura_models::webrtc::RtcOffer = serde_json::from_str(&delivered_bob.data).unwrap();
    assert_eq!(parsed.from_user_id, 1);
    assert_eq!(parsed.to_user_id, 2);

    // Bob -> Alice: answer should be forwarded to Alice
    let answer = rura_models::webrtc::RtcAnswer {
        from_user_id: 2,
        to_user_id: 1,
        sdp: "A".to_string(),
    };
    webrtc::process_answer(Arc::clone(&state), answer.clone()).await;
    let delivered_alice = timeout(Duration::from_millis(100), rx_alice.recv())
        .await
        .expect("answer timeout")
        .expect("answer channel closed");
    assert_eq!(delivered_alice.command, "rtc_answer");
    let parsed_a: rura_models::webrtc::RtcAnswer =
        serde_json::from_str(&delivered_alice.data).unwrap();
    assert_eq!(parsed_a.from_user_id, 2);
    assert_eq!(parsed_a.to_user_id, 1);

    // Session should be marked active
    assert!(webrtc::has_active_session(1, 2));
}
