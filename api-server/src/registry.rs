use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::id::SessionId;
use crate::session::Session;

/// The set of sessions currently running on this node.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, metadata: String) -> Session {
        let id = SessionId::generate();
        let session = Session::spawn(id.clone(), metadata, self.clone());
        self.lock().insert(id, session.clone());
        session
    }

    pub fn get(&self, id: &SessionId) -> Option<Session> {
        self.lock().get(id).cloned()
    }

    pub(crate) fn remove(&self, id: &SessionId) {
        self.lock().remove(id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Session>> {
        self.sessions.lock().expect("session registry poisoned")
    }
}
