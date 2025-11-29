use rura_server::client::handle_client;
use rura_server::messaging::state::AppState;
use rura_server::models::client_message::{AuthRequest, ClientMessage};
use rura_server::webrtc;
use rura_server::webrtc::handler::identity_to_session_id;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};
use tokio::time::{timeout, Duration};

fn test_socket_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9999)
}

fn stream_pair() -> (DuplexStream, DuplexStream) {
    duplex(4096)
}

async fn create_test_db() -> Arc<Mutex<Connection>> {
    let conn = Connection::open(":memory:").unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            passphrase TEXT NOT NULL UNIQUE,
            password TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            timestamp TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    Arc::new(Mutex::new(conn))
}

async fn read_msg(stream: &mut DuplexStream) -> ClientMessage {
    // Read a single newline-delimited JSON message from the stream.
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).await.unwrap();
        if n == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
    }
    let txt = String::from_utf8(buf).unwrap();
    serde_json::from_str(&txt).unwrap()
}

async fn write_json(stream: &mut DuplexStream, msg: &ClientMessage) {
    let mut line = serde_json::to_string(msg).unwrap();
    line.push('\n');
    stream.write_all(line.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

/// End-to-end integration over the full client protocol:
/// two clients register, exchange a call_invite/answer/hangup over the
/// JSON stream, and the server forwards signaling correctly.
#[tokio::test]
async fn call_signaling_end_to_end_over_stream() {
    // In-memory DB schema (users + connections) for this test
    let conn = create_test_db().await;
    let state = Arc::new(AppState::new(true));
    // Register WebRTC module (same as main)
    webrtc::register();

    // Create two in-process client/server stream pairs
    let (server_a, mut client_a) = stream_pair();
    let (server_b, mut client_b) = stream_pair();
    let addr = test_socket_addr();

    // Spawn handle_client for each "server side" stream
    {
        let db = Arc::clone(&conn);
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = handle_client(server_a, db, st, addr).await;
        });
    }
    {
        let db = Arc::clone(&conn);
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = handle_client(server_b, db, st, addr).await;
        });
    }

    // Both clients should receive auth_required
    let wrap_a = read_msg(&mut client_a).await;
    let wrap_b = read_msg(&mut client_b).await;
    assert_eq!(wrap_a.command, "auth_required");
    assert_eq!(wrap_b.command, "auth_required");

    // Register Alice
    let reg_a = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice-call".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut client_a, &reg_a).await;
    let wrap_a_resp = read_msg(&mut client_a).await;
    assert_eq!(wrap_a_resp.command, "auth_response");
    let val_a: serde_json::Value = serde_json::from_str(&wrap_a_resp.data).unwrap();
    let uid_a = if let Some(id_str) = val_a.get("id").and_then(|v| v.as_str()) {
        identity_to_session_id(id_str).expect("alice id -> session id")
    } else if let Some(u) = val_a.get("user_id").and_then(|v| v.as_i64()) {
        u
    } else {
        panic!("alice missing id/user_id in auth_response: {val_a}");
    };

    // Register Bob
    let reg_b = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob-call".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut client_b, &reg_b).await;
    let wrap_b_resp = read_msg(&mut client_b).await;
    assert_eq!(wrap_b_resp.command, "auth_response");
    let val_b: serde_json::Value = serde_json::from_str(&wrap_b_resp.data).unwrap();
    let uid_b = if let Some(id_str) = val_b.get("id").and_then(|v| v.as_str()) {
        identity_to_session_id(id_str).expect("bob id -> session id")
    } else if let Some(u) = val_b.get("user_id").and_then(|v| v.as_i64()) {
        u
    } else {
        panic!("bob missing id/user_id in auth_response: {val_b}");
    };

    // Alice -> Bob: send call_invite
    let invite = rura_models::webrtc::CallInvite {
        call_id: "call-itest-1".to_string(),
        from_user_id: uid_a,
        to_user_id: uid_b,
        media: rura_models::webrtc::CallMediaProfile {
            audio_enabled: true,
            video_enabled: false,
            audio_muted: Some(false),
            video_muted: Some(true),
        },
        preview: None,
        client: None,
        ringing_timeout_ms: Some(5_000),
    };
    let invite_msg = ClientMessage {
        command: "call_invite".into(),
        data: serde_json::to_string(&invite).unwrap(),
    };
    write_json(&mut client_a, &invite_msg).await;

    // Bob should see call_invite then call_ringing
    let delivered_invite = timeout(Duration::from_millis(200), read_msg(&mut client_b))
        .await
        .expect("invite timeout");
    assert_eq!(delivered_invite.command, "call_invite");
    let inv_body: rura_models::webrtc::CallInvite =
        serde_json::from_str(&delivered_invite.data).unwrap();
    assert_eq!(inv_body.call_id, invite.call_id);
    assert_eq!(inv_body.to_user_id, uid_b);

    let bob_ringing = timeout(Duration::from_millis(200), read_msg(&mut client_b))
        .await
        .expect("bob ringing timeout");
    assert_eq!(bob_ringing.command, "call_ringing");

    // Alice should see call_ringing as well
    let alice_ringing = timeout(Duration::from_millis(200), read_msg(&mut client_a))
        .await
        .expect("alice ringing timeout");
    assert_eq!(alice_ringing.command, "call_ringing");

    // Bob -> Alice: send call_answer
    let answer = rura_models::webrtc::CallAnswer {
        call_id: invite.call_id.clone(),
        from_user_id: uid_b,
        to_user_id: uid_a,
        resume_media: None,
    };
    let answer_msg = ClientMessage {
        command: "call_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    write_json(&mut client_b, &answer_msg).await;

    // Alice should receive call_answer
    let delivered_answer = timeout(Duration::from_millis(200), read_msg(&mut client_a))
        .await
        .expect("answer timeout");
    assert_eq!(delivered_answer.command, "call_answer");
    let ans_body: rura_models::webrtc::CallAnswer =
        serde_json::from_str(&delivered_answer.data).unwrap();
    assert_eq!(ans_body.call_id, invite.call_id);
    assert_eq!(ans_body.from_user_id, uid_b);
    assert_eq!(ans_body.to_user_id, uid_a);

    // Alice -> Bob: hang up
    let hang = rura_models::webrtc::CallHangup {
        call_id: invite.call_id.clone(),
        from_user_id: uid_a,
        to_user_id: uid_b,
        reason: Some(rura_models::webrtc::CallEndReason::Hangup),
    };
    let hang_msg = ClientMessage {
        command: "call_hangup".into(),
        data: serde_json::to_string(&hang).unwrap(),
    };
    write_json(&mut client_a, &hang_msg).await;

    // Bob should receive call_hangup
    let delivered_hang = timeout(Duration::from_millis(200), read_msg(&mut client_b))
        .await
        .expect("hangup timeout");
    assert_eq!(delivered_hang.command, "call_hangup");
    let hang_body: rura_models::webrtc::CallHangup =
        serde_json::from_str(&delivered_hang.data).unwrap();
    assert_eq!(hang_body.call_id, invite.call_id);
}

/// If a user already has an active call session, a new call_invite
/// from that user should be rejected and *not* delivered to the peer.
#[tokio::test]
async fn call_invite_rejected_when_user_busy() {
    let conn = create_test_db().await;
    let state = Arc::new(AppState::new(true));
    webrtc::register();

    let (server_a, mut client_a) = stream_pair();
    let (server_b, mut client_b) = stream_pair();
    let addr = test_socket_addr();

    {
        let db = Arc::clone(&conn);
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = handle_client(server_a, db, st, addr).await;
        });
    }
    {
        let db = Arc::clone(&conn);
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = handle_client(server_b, db, st, addr).await;
        });
    }

    // auth_required for both
    let _ = read_msg(&mut client_a).await;
    let _ = read_msg(&mut client_b).await;

    // Register Alice
    let reg_a = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "alice-busy".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut client_a, &reg_a).await;
    let wrap_a_resp = read_msg(&mut client_a).await;
    let val_a: serde_json::Value = serde_json::from_str(&wrap_a_resp.data).unwrap();
    let uid_a = if let Some(id_str) = val_a.get("id").and_then(|v| v.as_str()) {
        identity_to_session_id(id_str).expect("alice id -> session id")
    } else if let Some(u) = val_a.get("user_id").and_then(|v| v.as_i64()) {
        u
    } else {
        panic!("alice missing id/user_id in auth_response: {val_a}");
    };

    // Register Bob
    let reg_b = ClientMessage {
        command: "register".into(),
        data: serde_json::to_string(&AuthRequest {
            passphrase: "bob-busy".into(),
            password: "secret".into(),
            identity_key: None,
        })
        .unwrap(),
    };
    write_json(&mut client_b, &reg_b).await;
    let wrap_b_resp = read_msg(&mut client_b).await;
    let val_b: serde_json::Value = serde_json::from_str(&wrap_b_resp.data).unwrap();
    let uid_b = if let Some(id_str) = val_b.get("id").and_then(|v| v.as_str()) {
        identity_to_session_id(id_str).expect("bob id -> session id")
    } else if let Some(u) = val_b.get("user_id").and_then(|v| v.as_i64()) {
        u
    } else {
        panic!("bob missing id/user_id in auth_response: {val_b}");
    };

    // First call: Alice -> Bob (establish an active call)
    let invite_1 = rura_models::webrtc::CallInvite {
        call_id: "call-itest-busy-1".to_string(),
        from_user_id: uid_a,
        to_user_id: uid_b,
        media: rura_models::webrtc::CallMediaProfile {
            audio_enabled: true,
            video_enabled: false,
            audio_muted: Some(false),
            video_muted: Some(true),
        },
        preview: None,
        client: None,
        ringing_timeout_ms: Some(5_000),
    };
    let invite_msg_1 = ClientMessage {
        command: "call_invite".into(),
        data: serde_json::to_string(&invite_1).unwrap(),
    };
    write_json(&mut client_a, &invite_msg_1).await;

    // Bob receives invite + ringing; Alice receives ringing.
    let _ = timeout(Duration::from_millis(200), read_msg(&mut client_b))
        .await
        .expect("bob did not see first invite");
    let _ = timeout(Duration::from_millis(200), read_msg(&mut client_b))
        .await
        .expect("bob did not see first ringing");
    let _ = timeout(Duration::from_millis(200), read_msg(&mut client_a))
        .await
        .expect("alice did not see first ringing");

    // Bob answers, moving call into Connected state.
    let answer = rura_models::webrtc::CallAnswer {
        call_id: invite_1.call_id.clone(),
        from_user_id: uid_b,
        to_user_id: uid_a,
        resume_media: None,
    };
    let answer_msg = ClientMessage {
        command: "call_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    write_json(&mut client_b, &answer_msg).await;
    let _ = timeout(Duration::from_millis(200), read_msg(&mut client_a))
        .await
        .expect("alice did not see call_answer");

    // Second call attempt while first is active: should be rejected as UserBusy.
    let invite_2 = rura_models::webrtc::CallInvite {
        call_id: "call-itest-busy-2".to_string(),
        from_user_id: uid_a,
        to_user_id: uid_b,
        media: rura_models::webrtc::CallMediaProfile {
            audio_enabled: true,
            video_enabled: false,
            audio_muted: Some(false),
            video_muted: Some(true),
        },
        preview: None,
        client: None,
        ringing_timeout_ms: Some(5_000),
    };
    let invite_msg_2 = ClientMessage {
        command: "call_invite".into(),
        data: serde_json::to_string(&invite_2).unwrap(),
    };
    write_json(&mut client_a, &invite_msg_2).await;

    // Alice should receive an error about the rejected invite.
    let err_msg = timeout(Duration::from_millis(200), read_msg(&mut client_a))
        .await
        .expect("expected error for second invite");
    assert_eq!(err_msg.command, "error");
    assert!(
        err_msg
            .data
            .contains("call_invite rejected: user")
            && err_msg.data.contains("already has an active call"),
        "unexpected error payload: {}",
        err_msg.data
    );

    // Bob should *not* receive a second call_invite when server rejects it.
    let maybe_bob_second = timeout(Duration::from_millis(200), read_msg(&mut client_b)).await;
    assert!(
        maybe_bob_second.is_err(),
        "callee unexpectedly received a message for rejected invite: {:?}",
        maybe_bob_second
    );
}
