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

    let invite = rura_models::webrtc::CallInvite {
        call_id: "call-123".to_string(),
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
        ringing_timeout_ms: Some(1_000),
    };
    webrtc::process_call_invite(Arc::clone(&state), invite.clone())
        .await
        .expect("invite ok");
    let delivered_invite = timeout(Duration::from_millis(100), rx_bob.recv())
        .await
        .expect("invite timeout")
        .expect("invite channel closed");
    assert_eq!(delivered_invite.command, "call_invite");

    // Bob gets ringing update, as does Alice
    let bob_ring = timeout(Duration::from_millis(100), rx_bob.recv())
        .await
        .expect("bob ring timeout")
        .expect("ring channel closed");
    assert_eq!(bob_ring.command, "call_ringing");
    let alice_ring = timeout(Duration::from_millis(100), rx_alice.recv())
        .await
        .expect("alice ring timeout")
        .expect("ring channel closed");
    assert_eq!(alice_ring.command, "call_ringing");

    let answer = rura_models::webrtc::CallAnswer {
        call_id: invite.call_id.clone(),
        from_user_id: 2,
        to_user_id: 1,
        resume_media: None,
    };
    webrtc::process_call_answer(Arc::clone(&state), answer.clone())
        .await
        .expect("call answer");
    let delivered_answer = timeout(Duration::from_millis(100), rx_alice.recv())
        .await
        .expect("answer timeout")
        .expect("answer channel closed");
    assert_eq!(delivered_answer.command, "call_answer");

    // Alice -> Bob: offer should be forwarded to Bob
    let offer = rura_models::webrtc::RtcOffer {
        from_user_id: 1,
        to_user_id: 2,
        sdp: "O".to_string(),
        call_id: Some(invite.call_id.clone()),
    };
    webrtc::process_offer(Arc::clone(&state), offer.clone())
        .await
        .expect("offer ok");
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
        call_id: Some(invite.call_id.clone()),
    };
    webrtc::process_answer(Arc::clone(&state), answer.clone())
        .await
        .expect("rtc answer ok");
    let delivered_alice = timeout(Duration::from_millis(100), rx_alice.recv())
        .await
        .expect("answer timeout")
        .expect("answer channel closed");
    assert_eq!(delivered_alice.command, "rtc_answer");
    let parsed_a: rura_models::webrtc::RtcAnswer =
        serde_json::from_str(&delivered_alice.data).unwrap();
    assert_eq!(parsed_a.from_user_id, 2);
    assert_eq!(parsed_a.to_user_id, 1);

    // Session should be marked active via registry lookup
    let active = state
        .with_rtc_sessions(|registry| registry.touch_for_pair(Some(&invite.call_id), 1, 2).is_ok())
        .await;
    assert!(active);
}
