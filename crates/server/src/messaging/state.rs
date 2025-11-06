use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

use crate::models::client_message::ClientMessage;

#[derive(Clone)]
pub struct ClientHandle {
    pub tx: mpsc::UnboundedSender<ClientMessage>,
}

pub struct AppState {
    users: RwLock<HashMap<String, ClientHandle>>, // identity_key -> handle
    require_e2ee: bool,
}

impl AppState {
    pub fn new(require_e2ee: bool) -> Self {
        Self {
            users: RwLock::new(HashMap::new()),
            require_e2ee,
        }
    }

    pub async fn register(&self, identity_key: String, handle: ClientHandle) {
        let mut guard = self.users.write().await;
        guard.insert(identity_key, handle);
    }

    pub async fn unregister(&self, identity_key: &str) {
        let mut guard = self.users.write().await;
        guard.remove(identity_key);
    }

    pub async fn get_sender(
        &self,
        identity_key: &str,
    ) -> Option<mpsc::UnboundedSender<ClientMessage>> {
        let guard = self.users.read().await;
        guard.get(identity_key).map(|h| h.tx.clone())
    }

    pub fn require_e2ee(&self) -> bool {
        self.require_e2ee
    }
}

pub type SharedAppState = Arc<AppState>;

impl Default for AppState {
    fn default() -> Self {
        Self::new(true)
    }
}
