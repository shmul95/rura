use anyhow::{anyhow, Context, Result};
use rura_client::api::*;
use rura_client::local_storage;
use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// Simple CLI wrapper around rura_client for manual testing.
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "Usage: rura-cli <host> <port> <ca_pem_path> <password>\n\nExample: rura-cli 127.0.0.1 8443 certs/ca.crt secret",
        );
        std::process::exit(1);
    }
    let host = args[1].clone();
    let port: u16 = args[2].parse().context("invalid port")?;
    let ca_path = PathBuf::from(&args[3]);
    let password = args[4].clone();

    let ca_pem = fs::read_to_string(&ca_path)
        .with_context(|| format!("failed to read CA PEM at {:?}", ca_path))?;

    // Ensure local store initialized before using helpers
    local_storage::init_storage().map_err(|e| anyhow!("local storage: {e}"))?;

    // Open stream and get event receiver + user id
    let (user_id, rx) = open_message_stream_cli(host.clone(), port, ca_pem, String::new(), password)
        .map_err(|e| anyhow!("stream: {e}"))?;

    eprintln!("Connected as user {user_id}");

    // Spawn event printer thread
    std::thread::spawn(move || {
        print_events(rx);
    });

    println!("Type 'help' for commands. CLI is bound to user {user_id}.");
    let mut line = String::new();
    loop {
        print!("rura> ");
        let _ = io::stdout().flush();
        line.clear();
        if io::stdin().lock().read_line(&mut line).is_err() {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "quit" || trimmed == "exit" {
            break;
        }
        if trimmed == "help" {
            print_help();
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let rest: Vec<&str> = parts.collect();
        if let Err(e) = handle_command(cmd, &rest, user_id) {
            eprintln!("error: {e}");
        }
    }

    Ok(())
}

fn print_events(rx: Receiver<String>) {
    while let Ok(line) = rx.recv() {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
                match t {
                    "auth_ok" => {}
                    "call_invite" | "call_ringing" | "call_answer" | "call_reject" | "call_hangup" => {
                        eprintln!("[event] {}: {}", t, v);
                    }
                    "rtc_media" => {
                        eprintln!("[media] {}", v);
                    }
                    _ => {
                        eprintln!("[event] {}", v);
                    }
                }
            } else {
                eprintln!("[event] {}", v);
            }
        } else {
            eprintln!("[event] {}", line);
        }
    }
}

fn print_help() {
    println!("Commands:");
    println!("  help                                Show this help");
    println!("  quit | exit                        Quit CLI");
    println!("  add-contact <id_b64> <pubkey_b64> [nickname]");
    println!("  list-contacts");
    println!("  msg-id <user_id> <text>");
    println!("  msg-identity <id_b64> <text>");
    println!("  send-file-identity <id_b64> <path>");
    println!("  start-call-id <user_id> [video]");
    println!("  start-call-identity <id_b64> [video]");
    println!("  accept-call <call_id> [video]");
    println!("  reject-call <call_id> [busy]");
    println!("  end-call <call_id>");
    println!("  show-call");
}

fn handle_command(cmd: &str, args: &[&str], user_id: i64) -> Result<()> {
    match cmd {
        "add-contact" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "usage: add-contact <id_b64> <pubkey_b64> [nickname]"
                ));
            }
            let identity = args[0].to_string();
            let pubkey = args[1].to_string();
            let nickname = args.get(2).map(|s| s.to_string());
            add_contact_with_nickname(identity, pubkey, nickname)
                .map_err(|e| anyhow!("add_contact: {e}"))?;
            println!("contact added");
        }
        "list-contacts" => {
            let json = list_contacts_json()
                .map_err(|e| anyhow!("list_contacts: {e}"))?;
            println!("{}", json);
        }
        "msg-id" => {
            if args.len() < 2 {
                return Err(anyhow!("usage: msg-id <user_id> <text>"));
            }
            let to: i64 = args[0]
                .parse()
                .map_err(|_| anyhow!("invalid user_id: {}", args[0]))?;
            let body = args[1..].join(" ");
            send_direct_message_over_stream(user_id, to, body)
                .map_err(|e| anyhow!("send: {e}"))?;
            println!("sent");
        }
        "msg-identity" => {
            if args.len() < 2 {
                return Err(anyhow!("usage: msg-identity <id_b64> <text>"));
            }
            let to = args[0].to_string();
            let body = args[1..].join(" ");
            send_direct_message_over_stream_to_identity(user_id, to, body)
                .map_err(|e| anyhow!("send: {e}"))?;
            println!("sent");
        }
        "send-file-identity" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "usage: send-file-identity <id_b64> <path>"
                ));
            }
            let to = args[0].to_string();
            let path = PathBuf::from(args[1]);
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read {:?}", path))?;
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file")
                .to_string();
            let mime = guess_mime(&name);
            send_media_to_identity(
                user_id,
                to,
                mime.to_string(),
                Some(name),
                bytes,
                Some(12 * 1024),
            )
            .map_err(|e| anyhow!("send media: {e}"))?;
            println!("file sent");
        }
        "start-call-id" => {
            if args.is_empty() {
                return Err(anyhow!(
                    "usage: start-call-id <user_id> [video]"
                ));
            }
            let to: i64 = args[0]
                .parse()
                .map_err(|_| anyhow!("invalid user_id: {}", args[0]))?;
            let video = args
                .get(1)
                .map(|s| *s == "video" || *s == "true")
                .unwrap_or(false);
            let state = start_call(user_id, to, video)
                .map_err(|e| anyhow!("start_call: {e}"))?;
            println!(
                "call started: {} -> {} (id={})",
                user_id, to, state.call_id
            );
        }
        "start-call-identity" => {
            if args.is_empty() {
                return Err(anyhow!(
                    "usage: start-call-identity <id_b64> [video]"
                ));
            }
            let id_b64 = args[0];
            let video = args
                .get(1)
                .map(|s| *s == "video" || *s == "true")
                .unwrap_or(false);
            let remote = identity_to_numeric(id_b64)?;
            let state = start_call(user_id, remote, video)
                .map_err(|e| anyhow!("start_call: {e}"))?;
            println!(
                "call started: {} -> {} (id={})",
                user_id, remote, state.call_id
            );
        }
        "accept-call" => {
            if args.is_empty() {
                return Err(anyhow!(
                    "usage: accept-call <call_id> [video]"
                ));
            }
            let call_id = args[0].to_string();
            let video = args
                .get(1)
                .map(|s| *s == "video" || *s == "true")
                .unwrap_or(false);
            let state = accept_call(user_id, call_id.clone(), video)
                .map_err(|e| anyhow!("accept_call: {e}"))?;
            println!("call accepted (id={})", state.call_id);
        }
        "reject-call" => {
            if args.is_empty() {
                return Err(anyhow!(
                    "usage: reject-call <call_id> [busy]"
                ));
            }
            let call_id = args[0].to_string();
            let busy = args
                .get(1)
                .map(|s| *s == "busy" || *s == "true")
                .unwrap_or(false);
            reject_call(user_id, call_id, busy)
                .map_err(|e| anyhow!("reject_call: {e}"))?;
            println!("call rejected");
        }
        "end-call" => {
            if args.is_empty() {
                return Err(anyhow!("usage: end-call <call_id>"));
            }
            let call_id = args[0].to_string();
            end_call(user_id, call_id).map_err(|e| anyhow!("end_call: {e}"))?;
            println!("call ended");
        }
        "show-call" => {
            let state = get_current_call_state()
                .map_err(|e| anyhow!("get_current_call_state: {e}"))?;
            println!("{:?}", state);
        }
        other => {
            return Err(anyhow!("unknown command: {}", other));
        }
    }
    Ok(())
}

fn guess_mime(name: &str) -> &str {
    let lower = name.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".pdf") {
        "application/pdf"
    } else if lower.ends_with(".txt") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

fn identity_to_numeric(id_b64: &str) -> Result<i64> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(id_b64)
        .map_err(|e| anyhow!("bad id base64: {e}"))?;
    if bytes.len() < 8 {
        return Err(anyhow!("identity too short"));
    }
    let mut slice = [0u8; 8];
    slice.copy_from_slice(&bytes[0..8]);
    let v = u64::from_be_bytes(slice) & 0x7FFF_FFFF_FFFF_FFFF; // positive 63-bit
    Ok(v as i64)
}
