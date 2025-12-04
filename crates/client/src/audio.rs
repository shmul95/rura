// Audio for calls is currently stubbed out across all platforms.
// This keeps the client compiling cleanly (especially on Android/NixOS)
// while we focus on messaging and signaling.

pub fn start_call_audio(_remote_user_id: i64) -> Result<(), String> {
    Ok(())
}

pub fn stop_call_audio(_remote_user_id: i64) {}
