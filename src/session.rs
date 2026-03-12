use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::TokenUsage;

/// Session metadata stored in the session store
#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub session_id: Uuid,
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub workdir: PathBuf,
    pub auto_workdir: bool,
    pub task_count: u32,
    pub tokens: TokenUsage,
    pub cost_usd: f64,
    pub key_id: String,
}

/// Thread-safe session store using DashMap with per-session mutexes
pub struct SessionStore {
    sessions: DashMap<Uuid, SessionMeta>,
    locks: DashMap<Uuid, Arc<Mutex<()>>>,
}

impl SessionStore {
    /// Create a new session store
    pub fn new() -> Self {
        SessionStore {
            sessions: DashMap::new(),
            locks: DashMap::new(),
        }
    }

    /// Insert a session metadata into the store
    pub fn insert(&self, meta: SessionMeta) {
        let session_id = meta.session_id;
        self.sessions.insert(session_id, meta);
        self.locks.insert(session_id, Arc::new(Mutex::new(())));
    }

    /// Get a session metadata by ID
    pub fn get(&self, id: &Uuid) -> Option<SessionMeta> {
        self.sessions.get(id).map(|entry| entry.clone())
    }

    /// Update a session metadata using a closure
    pub fn update<F: FnOnce(&mut SessionMeta)>(&self, id: &Uuid, f: F) -> bool {
        if let Some(mut entry) = self.sessions.get_mut(id) {
            f(&mut entry);
            true
        } else {
            false
        }
    }

    /// Remove a session metadata by ID
    pub fn remove(&self, id: &Uuid) -> Option<SessionMeta> {
        self.locks.remove(id);
        self.sessions.remove(id).map(|(_, meta)| meta)
    }

    /// List all session metadata entries
    pub fn list_all(&self) -> Vec<SessionMeta> {
        self.sessions.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get the lock for a session
    pub fn get_lock(&self, id: &Uuid) -> Option<Arc<Mutex<()>>> {
        self.locks.get(id).map(|entry| Arc::clone(&entry))
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_crud() {
        let store = SessionStore::new();
        let session_id = Uuid::new_v4();
        let now = Utc::now();

        // Create a session
        let meta = SessionMeta {
            session_id,
            claude_session_id: Some("claude-123".to_string()),
            created_at: now,
            last_used: now,
            model: Some("claude-3-5-sonnet".to_string()),
            system_prompt: Some("You are helpful".to_string()),
            workdir: PathBuf::from("/tmp"),
            auto_workdir: false,
            task_count: 0,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
            key_id: "test".to_string(),
        };

        // Insert
        store.insert(meta.clone());

        // Get and verify exists
        let retrieved = store.get(&session_id);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.session_id, session_id);
        assert_eq!(retrieved.task_count, 0);

        // Update (increment task_count)
        let updated = store.update(&session_id, |m| {
            m.task_count += 1;
        });
        assert!(updated);

        // Get and verify updated
        let retrieved = store.get(&session_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().task_count, 1);

        // Remove
        let removed = store.remove(&session_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().task_count, 1);

        // Get and verify None
        let retrieved = store.get(&session_id);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_session_lock_exists() {
        let store = SessionStore::new();
        let session_id = Uuid::new_v4();
        let now = Utc::now();

        let meta = SessionMeta {
            session_id,
            claude_session_id: None,
            created_at: now,
            last_used: now,
            model: None,
            system_prompt: None,
            workdir: PathBuf::from("/tmp"),
            auto_workdir: false,
            task_count: 0,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
            key_id: "test".to_string(),
        };

        // Insert
        store.insert(meta);

        // Verify get_lock returns Some
        let lock = store.get_lock(&session_id);
        assert!(lock.is_some());

        // Remove
        store.remove(&session_id);

        // Verify get_lock returns None
        let lock = store.get_lock(&session_id);
        assert!(lock.is_none());
    }

    #[test]
    fn test_session_stores_key_id() {
        let store = SessionStore::new();
        let session_id = Uuid::new_v4();
        let now = Utc::now();
        let meta = SessionMeta {
            session_id,
            claude_session_id: None,
            created_at: now,
            last_used: now,
            model: None,
            system_prompt: None,
            workdir: PathBuf::from("/tmp"),
            auto_workdir: false,
            task_count: 0,
            tokens: TokenUsage::default(),
            cost_usd: 0.0,
            key_id: "admin".to_string(),
        };
        store.insert(meta);
        let retrieved = store.get(&session_id).unwrap();
        assert_eq!(retrieved.key_id, "admin");
    }

    #[test]
    fn test_list_all_sessions() {
        let store = SessionStore::new();
        let now = Utc::now();
        for i in 0..3 {
            let meta = SessionMeta {
                session_id: Uuid::new_v4(),
                claude_session_id: None,
                created_at: now,
                last_used: now,
                model: Some("sonnet".to_string()),
                system_prompt: None,
                workdir: PathBuf::from("/tmp"),
                auto_workdir: false,
                task_count: i,
                tokens: TokenUsage::default(),
                cost_usd: 0.0,
                key_id: "admin".to_string(),
            };
            store.insert(meta);
        }
        let all = store.list_all();
        assert_eq!(all.len(), 3);
    }
}
