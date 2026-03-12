use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use uuid::Uuid;

const SESSION_TTL_HOURS: i64 = 1;

#[derive(Debug, Clone)]
pub struct AdminSession {
    pub expires_at: DateTime<Utc>,
}

pub struct AdminSessionStore {
    pub sessions: DashMap<String, AdminSession>,
}

impl AdminSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn create_session(&self) -> String {
        self.cleanup_expired();
        let token = Uuid::new_v4().to_string();
        let session = AdminSession {
            expires_at: Utc::now() + Duration::hours(SESSION_TTL_HOURS),
        };
        self.sessions.insert(token.clone(), session);
        token
    }

    pub fn validate(&self, token: &str) -> bool {
        if let Some(session) = self.sessions.get(token) {
            session.expires_at > Utc::now()
        } else {
            false
        }
    }

    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        self.sessions.retain(|_, session| session.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_admin_session() {
        let store = AdminSessionStore::new();
        let token = store.create_session();
        assert!(store.validate(&token));
    }

    #[test]
    fn test_invalid_token_rejected() {
        let store = AdminSessionStore::new();
        assert!(!store.validate("invalid-token"));
    }

    #[test]
    fn test_expired_session_rejected() {
        let store = AdminSessionStore::new();
        let token = Uuid::new_v4().to_string();
        store.sessions.insert(
            token.clone(),
            AdminSession {
                expires_at: Utc::now() - Duration::hours(2),
            },
        );
        assert!(!store.validate(&token));
    }

    #[test]
    fn test_cleanup_removes_expired() {
        let store = AdminSessionStore::new();
        let expired_token = Uuid::new_v4().to_string();
        store.sessions.insert(
            expired_token.clone(),
            AdminSession {
                expires_at: Utc::now() - Duration::hours(2),
            },
        );
        let valid_token = store.create_session();
        store.cleanup_expired();
        assert_eq!(store.sessions.len(), 1);
        assert!(store.validate(&valid_token));
    }
}
