//! The encrypted session cookie: a Fernet token wrapping
//! {"created": <ts>, "user": <user id>}.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use http::{header, HeaderMap};
use serde_json::{json, Value};

use crate::fernet::Fernet;

pub const COOKIE_NAME: &str = "session";

#[derive(Clone)]
pub struct SessionStore {
    fernet: Arc<Fernet>,
}

#[derive(Debug, Default)]
pub struct Session {
    pub created: Option<i64>,
    user: Option<i64>,
}

impl Session {
    pub fn user_id(&self) -> Option<i64> {
        self.user
    }

    pub fn remember(&mut self, user_id: i64) {
        self.user = Some(user_id);
    }

    pub fn forget(&mut self) {
        self.user = None;
    }

    pub fn has_identity(&self) -> bool {
        self.user.is_some()
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl SessionStore {
    pub fn new(key: &[u8]) -> Option<Self> {
        Some(SessionStore {
            fernet: Arc::new(Fernet::new(key)?),
        })
    }

    pub fn load(&self, headers: &HeaderMap) -> Session {
        let Some(token) = cookie_value(headers, COOKIE_NAME) else {
            return Session::default();
        };
        let Some(plaintext) = self.fernet.decrypt(&token) else {
            return Session::default();
        };
        let Ok(value) = serde_json::from_slice::<Value>(&plaintext) else {
            return Session::default();
        };
        let created = value.get("created").and_then(Value::as_i64);
        let user = value.get("user").and_then(Value::as_i64);
        Session { created, user }
    }

    /// Set-Cookie header value saving the session (value quoted because
    /// of base64 '=' padding).
    pub fn save_cookie(&self, session: &Session) -> String {
        let created = session.created.unwrap_or_else(now);
        let payload = json!({
            "created": created,
            "user": session.user,
        });
        let mut iv = [0u8; 16];
        let _ = getrandom::getrandom(&mut iv);
        let token = self
            .fernet
            .encrypt(payload.to_string().as_bytes(), now() as u64, iv);
        format!("{COOKIE_NAME}=\"{token}\"; HttpOnly; Path=/")
    }

    /// Set-Cookie header value clearing the session.
    pub fn clear_cookie(&self) -> String {
        format!(
            "{COOKIE_NAME}=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; HttpOnly; Path=/"
        )
    }
}

/// Extract a cookie value from request headers, handling optional quoting.
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for header in headers.get_all(header::COOKIE) {
        let Ok(value) = header.to_str() else { continue };
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some((key, val)) = pair.split_once('=') {
                if key == name {
                    let val = val.trim_matches('"');
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        SessionStore::new(&[7u8; 32]).unwrap()
    }

    fn headers_with_cookie(store: &SessionStore, payload: &str) -> HeaderMap {
        let mut iv = [0u8; 16];
        let _ = getrandom::getrandom(&mut iv);
        let token = store.fernet.encrypt(payload.as_bytes(), now() as u64, iv);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{COOKIE_NAME}=\"{token}\"").parse().unwrap(),
        );
        headers
    }

    #[test]
    fn round_trip() {
        let store = store();
        let mut session = Session::default();
        session.remember(42);
        let cookie = store.save_cookie(&session);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            cookie.split(';').next().unwrap().parse().unwrap(),
        );
        let loaded = store.load(&headers);
        assert_eq!(loaded.user_id(), Some(42));
        assert!(loaded.has_identity());
    }

    /// A cookie in the legacy nested payload format decrypts but carries
    /// no identity: the user is asked to log in again, nothing errors.
    #[test]
    fn legacy_payload_degrades_to_anonymous() {
        let store = store();
        let headers = headers_with_cookie(
            &store,
            "{\"created\": 1700000000, \"session\": {\"AIOHTTP_SECURITY\": \"1\"}}",
        );
        let session = store.load(&headers);
        assert!(!session.has_identity());
        assert_eq!(session.user_id(), None);
        assert_eq!(session.created, Some(1700000000));
    }
}
