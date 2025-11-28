use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::messaging::state::AppState;
use crate::models::client_message::ClientMessage;
use base64::Engine as _;
use rura_models::webrtc::{
    CallAnswer, CallEndReason, CallHangup, CallInvite, CallReject, CallRinging, IceCandidate,
    RtcAnswer, RtcOffer,
};

const DEFAULT_RING_TIMEOUT_MS: u64 = 30_000;
const MAX_RING_TIMEOUT_MS: u64 = 90_000;
const CONNECTED_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const ENDED_RETENTION: Duration = Duration::from_secs(5);

/// Call at server start to ensure the module is linked and ready.
pub fn register() {
    println!("[webrtc] signaling module registered");
}

fn ordered_pair(a: i64, b: i64) -> (i64, i64) {
    if a <= b { (a, b) } else { (b, a) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSessionState {
    Ringing,
    Connected,
    Ended,
}

#[derive(Debug, Clone)]
pub struct CallSessionMeta {
    pub call_id: String,
    pub initiator: i64,
    pub callee: i64,
    pub state: CallSessionState,
    pub last_activity: SystemTime,
    pub cleanup_deadline: Option<SystemTime>,
}

impl CallSessionMeta {
    fn is_active(&self) -> bool {
        !matches!(self.state, CallSessionState::Ended)
    }

    fn includes(&self, user_id: i64) -> bool {
        self.initiator == user_id || self.callee == user_id
    }
}

#[derive(Default)]
pub struct RtcSessionManager {
    sessions: HashMap<(i64, i64), CallSessionMeta>,
    call_ids: HashMap<String, (i64, i64)>,
}

impl RtcSessionManager {
    pub fn create_invite(
        &mut self,
        invite: &CallInvite,
        ring_deadline: SystemTime,
    ) -> Result<CallSessionMeta, CallError> {
        if invite.from_user_id == invite.to_user_id {
            return Err(CallError::SelfCall);
        }
        if self.user_busy(invite.from_user_id) {
            return Err(CallError::UserBusy(invite.from_user_id));
        }
        if self.user_busy(invite.to_user_id) {
            return Err(CallError::UserBusy(invite.to_user_id));
        }
        let key = ordered_pair(invite.from_user_id, invite.to_user_id);
        let meta = CallSessionMeta {
            call_id: invite.call_id.clone(),
            initiator: invite.from_user_id,
            callee: invite.to_user_id,
            state: CallSessionState::Ringing,
            last_activity: SystemTime::now(),
            cleanup_deadline: Some(ring_deadline),
        };
        self.call_ids.insert(meta.call_id.clone(), key);
        self.sessions.insert(key, meta.clone());
        Ok(meta)
    }

    pub fn mark_connected(
        &mut self,
        call_id: &str,
        actor: i64,
    ) -> Result<CallSessionMeta, CallError> {
        let (_, meta) = self.get_mut_by_call(call_id)?;
        if !meta.includes(actor) {
            return Err(CallError::NotParticipant);
        }
        if matches!(meta.state, CallSessionState::Ended) {
            return Err(CallError::AlreadyEnded);
        }
        meta.state = CallSessionState::Connected;
        meta.last_activity = SystemTime::now();
        meta.cleanup_deadline = None;
        Ok(meta.clone())
    }

    pub fn mark_rejected(
        &mut self,
        call_id: &str,
        actor: i64,
        _reason: CallEndReason,
    ) -> Result<CallSessionMeta, CallError> {
        let (_, meta) = self.get_mut_by_call(call_id)?;
        if !meta.includes(actor) {
            return Err(CallError::NotParticipant);
        }
        Ok(Self::finish_call(meta))
    }

    pub fn mark_hangup(
        &mut self,
        call_id: &str,
        actor: i64,
        _reason: CallEndReason,
    ) -> Result<CallSessionMeta, CallError> {
        let (_, meta) = self.get_mut_by_call(call_id)?;
        if !meta.includes(actor) {
            return Err(CallError::NotParticipant);
        }
        Ok(Self::finish_call(meta))
    }

    pub fn touch_for_pair(
        &mut self,
        call_id: Option<&str>,
        a: i64,
        b: i64,
    ) -> Result<CallSessionMeta, CallError> {
        let key = if let Some(id) = call_id {
            self.call_ids
                .get(id)
                .copied()
                .ok_or(CallError::UnknownCall)?
        } else {
            ordered_pair(a, b)
        };
        let meta = self.sessions.get_mut(&key).ok_or(CallError::UnknownCall)?;
        if !meta.includes(a) {
            return Err(CallError::NotParticipant);
        }
        if matches!(meta.state, CallSessionState::Ended) {
            return Err(CallError::AlreadyEnded);
        }
        meta.last_activity = SystemTime::now();
        Ok(meta.clone())
    }

    pub fn ensure_pair_session(&mut self, a: i64, b: i64) {
        let key = ordered_pair(a, b);
        if self.sessions.contains_key(&key) {
            return;
        }
        let call_id = format!("legacy-{}-{}", key.0, key.1);
        let meta = CallSessionMeta {
            call_id: call_id.clone(),
            initiator: a,
            callee: b,
            state: CallSessionState::Connected,
            last_activity: SystemTime::now(),
            cleanup_deadline: None,
        };
        self.call_ids.insert(call_id, key);
        self.sessions.insert(key, meta);
    }

    pub fn cleanup(&mut self) -> Vec<(CallSessionMeta, CallEndReason)> {
        let now = SystemTime::now();
        let mut timed_out = Vec::new();
        let mut to_remove = Vec::new();
        for (key, meta) in self.sessions.iter_mut() {
            match meta.state {
                CallSessionState::Ringing => {
                    if let Some(deadline) = meta.cleanup_deadline
                        && deadline <= now
                    {
                        let clone = Self::finish_call(meta);
                        timed_out.push((clone.clone(), CallEndReason::Timeout));
                    }
                }
                CallSessionState::Connected => {
                    if let Ok(elapsed) = now.duration_since(meta.last_activity)
                        && elapsed > CONNECTED_IDLE_TIMEOUT
                    {
                        let clone = Self::finish_call(meta);
                        timed_out.push((clone.clone(), CallEndReason::Timeout));
                    }
                }
                CallSessionState::Ended => {
                    if let Some(deadline) = meta.cleanup_deadline
                        && deadline <= now
                    {
                        to_remove.push((*key, meta.call_id.clone()));
                    }
                }
            }
        }
        for (key, call_id) in to_remove {
            self.sessions.remove(&key);
            self.call_ids.remove(&call_id);
        }
        timed_out
    }

    fn finish_call(meta: &mut CallSessionMeta) -> CallSessionMeta {
        meta.state = CallSessionState::Ended;
        meta.last_activity = SystemTime::now();
        meta.cleanup_deadline = Some(meta.last_activity + ENDED_RETENTION);
        meta.clone()
    }

    fn get_mut_by_call(
        &mut self,
        call_id: &str,
    ) -> Result<((i64, i64), &mut CallSessionMeta), CallError> {
        let key = self
            .call_ids
            .get(call_id)
            .copied()
            .ok_or(CallError::UnknownCall)?;
        let meta = self.sessions.get_mut(&key).ok_or(CallError::UnknownCall)?;
        Ok((key, meta))
    }

    fn user_busy(&self, user_id: i64) -> bool {
        self.sessions
            .values()
            .any(|meta| meta.is_active() && (meta.initiator == user_id || meta.callee == user_id))
    }
}

#[derive(Debug)]
pub enum CallError {
    SelfCall,
    UserBusy(i64),
    PeerOffline(i64),
    UnknownCall,
    NotParticipant,
    AlreadyEnded,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::SelfCall => write!(f, "cannot call yourself"),
            CallError::UserBusy(id) => write!(f, "user {id} already has an active call"),
            CallError::PeerOffline(id) => write!(f, "user {id} is offline"),
            CallError::UnknownCall => write!(f, "unknown or expired call"),
            CallError::NotParticipant => write!(f, "user not part of call"),
            CallError::AlreadyEnded => write!(f, "call already ended"),
        }
    }
}

impl std::error::Error for CallError {}

async fn send_if_online(state: &AppState, to_user_id: i64, msg: ClientMessage) {
    if let Some(tx) = state.get_sender_by_session_id(to_user_id).await {
        let _ = tx.send(msg);
    }
}

fn ring_deadline_ms(invite: &CallInvite) -> SystemTime {
    let ms = invite
        .ringing_timeout_ms
        .map(|v| v as u64)
        .unwrap_or(DEFAULT_RING_TIMEOUT_MS)
        .min(MAX_RING_TIMEOUT_MS);
    SystemTime::now() + Duration::from_millis(ms)
}

fn epoch_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

async fn cleanup_stale_sessions(state: &AppState) {
    let expired = state.with_rtc_sessions(|registry| registry.cleanup()).await;
    for (meta, reason) in expired {
        notify_call_end(state, &meta, reason).await;
    }
}

async fn notify_call_end(state: &AppState, meta: &CallSessionMeta, reason: CallEndReason) {
    let payload = CallHangup {
        call_id: meta.call_id.clone(),
        from_user_id: meta.initiator,
        to_user_id: meta.callee,
        reason: Some(reason),
    };
    let msg = ClientMessage {
        command: "call_hangup".into(),
        data: serde_json::to_string(&payload).unwrap(),
    };
    send_if_online(state, meta.initiator, msg.clone()).await;
    send_if_online(state, meta.callee, msg).await;
}

pub async fn process_call_invite(
    state: Arc<AppState>,
    invite: CallInvite,
) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    let initiator = invite.from_user_id;
    let callee = invite.to_user_id;
    let callee_online = state.get_sender_by_session_id(callee).await.is_some();
    if !callee_online {
        return Err(CallError::PeerOffline(callee));
    }
    let deadline = ring_deadline_ms(&invite);
    state
        .with_rtc_sessions(|registry| registry.create_invite(&invite, deadline))
        .await?;
    let invite_msg = ClientMessage {
        command: "call_invite".into(),
        data: serde_json::to_string(&invite).unwrap(),
    };
    send_if_online(&state, callee, invite_msg).await;
    let ringing = CallRinging {
        call_id: invite.call_id.clone(),
        callee_user_id: callee,
        ringing: true,
        expires_at: Some(epoch_ms(deadline)),
    };
    let ring_msg = ClientMessage {
        command: "call_ringing".into(),
        data: serde_json::to_string(&ringing).unwrap(),
    };
    send_if_online(&state, callee, ring_msg.clone()).await;
    send_if_online(&state, initiator, ring_msg).await;
    Ok(())
}

pub async fn process_call_answer(
    state: Arc<AppState>,
    answer: CallAnswer,
) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    state
        .with_rtc_sessions(|registry| registry.mark_connected(&answer.call_id, answer.from_user_id))
        .await?;
    let msg = ClientMessage {
        command: "call_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    send_if_online(&state, answer.to_user_id, msg).await;
    Ok(())
}

pub async fn process_call_reject(
    state: Arc<AppState>,
    reject: CallReject,
) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    let reason = reject.reason.clone().unwrap_or(CallEndReason::Rejected);
    state
        .with_rtc_sessions(|registry| {
            registry.mark_rejected(&reject.call_id, reject.from_user_id, reason)
        })
        .await?;
    let msg = ClientMessage {
        command: "call_reject".into(),
        data: serde_json::to_string(&reject).unwrap(),
    };
    send_if_online(&state, reject.to_user_id, msg.clone()).await;
    send_if_online(&state, reject.from_user_id, msg).await;
    Ok(())
}

pub async fn process_call_hangup(
    state: Arc<AppState>,
    hangup: CallHangup,
) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    let reason = hangup.reason.clone().unwrap_or(CallEndReason::Hangup);
    state
        .with_rtc_sessions(|registry| {
            registry.mark_hangup(&hangup.call_id, hangup.from_user_id, reason)
        })
        .await?;
    let msg = ClientMessage {
        command: "call_hangup".into(),
        data: serde_json::to_string(&hangup).unwrap(),
    };
    send_if_online(&state, hangup.to_user_id, msg.clone()).await;
    send_if_online(&state, hangup.from_user_id, msg).await;
    Ok(())
}

pub async fn process_offer(state: Arc<AppState>, offer: RtcOffer) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    if offer.call_id.is_none() {
        state
            .with_rtc_sessions(|registry| {
                registry.ensure_pair_session(offer.from_user_id, offer.to_user_id);
            })
            .await;
    }
    ensure_call_active(
        &state,
        offer.call_id.as_deref(),
        offer.from_user_id,
        offer.to_user_id,
    )
    .await?;
    let wrapper = ClientMessage {
        command: "rtc_offer".into(),
        data: serde_json::to_string(&offer).unwrap(),
    };
    send_if_online(&state, offer.to_user_id, wrapper).await;
    Ok(())
}

pub async fn process_answer(state: Arc<AppState>, answer: RtcAnswer) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    if answer.call_id.is_none() {
        state
            .with_rtc_sessions(|registry| {
                registry.ensure_pair_session(answer.from_user_id, answer.to_user_id);
            })
            .await;
    }
    ensure_call_active(
        &state,
        answer.call_id.as_deref(),
        answer.from_user_id,
        answer.to_user_id,
    )
    .await?;
    let wrapper = ClientMessage {
        command: "rtc_answer".into(),
        data: serde_json::to_string(&answer).unwrap(),
    };
    send_if_online(&state, answer.to_user_id, wrapper).await;
    Ok(())
}

pub async fn process_ice(state: Arc<AppState>, ice: IceCandidate) -> Result<(), CallError> {
    cleanup_stale_sessions(&state).await;
    if ice.call_id.is_none() {
        state
            .with_rtc_sessions(|registry| {
                registry.ensure_pair_session(ice.from_user_id, ice.to_user_id);
            })
            .await;
    }
    ensure_call_active(
        &state,
        ice.call_id.as_deref(),
        ice.from_user_id,
        ice.to_user_id,
    )
    .await?;
    let wrapper = ClientMessage {
        command: "rtc_ice".into(),
        data: serde_json::to_string(&ice).unwrap(),
    };
    send_if_online(&state, ice.to_user_id, wrapper).await;
    Ok(())
}

async fn ensure_call_active(
    state: &AppState,
    call_id: Option<&str>,
    from: i64,
    to: i64,
) -> Result<CallSessionMeta, CallError> {
    state
        .with_rtc_sessions(|registry| registry.touch_for_pair(call_id, from, to))
        .await
}

/// Convert a base64 identity string to a stable positive i64 for RTC session keys.
pub fn identity_to_session_id(id_b64: &str) -> Option<i64> {
    if let Ok(n) = id_b64.parse::<i64>() {
        return Some(n);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(id_b64)
        .ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let mut slice = [0u8; 8];
    slice.copy_from_slice(&bytes[0..8]);
    let v = u64::from_be_bytes(slice) & 0x7FFF_FFFF_FFFF_FFFF;
    Some(v as i64)
}
