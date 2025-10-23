use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use crate::models::client_message::ClientMessage;

#[derive(Clone)]
pub struct ClientHandle {
    pub tx: mpsc::UnboundedSender<ClientMessage>,
}

pub struct AppState {
    users: RwLock<HashMap<i64, ClientHandle>>, // user_id -> handle
    require_e2ee: bool,
}

impl AppState {
    pub fn new(require_e2ee: bool) -> Self {
        Self {
            users: RwLock::new(HashMap::new()),
            require_e2ee,
        }
    }

    // Backward-compatible default now enforces E2EE
    pub fn default() -> Self {
        Self::new(true)
    }

    pub async fn register(&self, user_id: i64, handle: ClientHandle) {
        let mut guard = self.users.write().await;
        guard.insert(user_id, handle);
    }

    pub async fn unregister(&self, user_id: i64) {
        let mut guard = self.users.write().await;
        guard.remove(&user_id);
    }

    pub async fn get_sender(&self, user_id: i64) -> Option<mpsc::UnboundedSender<ClientMessage>> {
        let guard = self.users.read().await;
        guard.get(&user_id).map(|h| h.tx.clone())
    }

    pub fn require_e2ee(&self) -> bool {
        self.require_e2ee
    }
}

pub type SharedAppState = Arc<AppState>;
