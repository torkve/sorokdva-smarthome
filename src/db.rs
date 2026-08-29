//! The data store. The whole database is a handful of records (one or two
//! users, one OAuth client, a few tokens), so it lives in memory and is
//! persisted as a single JSON file with an atomic write on every mutation.
//! Existing sqlite databases are converted once with scripts/migrate-db.py.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Split on the line separators that actually occur in the DB (\r\n,
/// \n, \r): interior empty lines are kept, a trailing newline is not.
pub fn splitlines(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut parts: Vec<String> = normalized.split('\n').map(str::to_string).collect();
    if normalized.ends_with('\n') {
        parts.pop();
    }
    parts
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Client {
    pub id: i64,
    pub user_id: Option<i64>,
    pub client_id: String,
    pub client_secret: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub redirect_uri: String,
    pub token_endpoint_auth_method: String,
    pub grant_type: String,
    pub response_type: String,
    pub scope: String,
    pub client_name: String,
    pub client_uri: String,
}

impl Client {
    pub fn redirect_uris(&self) -> Vec<String> {
        splitlines(&self.redirect_uri)
    }

    pub fn grant_types(&self) -> Vec<String> {
        splitlines(&self.grant_type)
    }

    pub fn response_types(&self) -> Vec<String> {
        splitlines(&self.response_type)
    }

    pub fn default_redirect_uri(&self) -> Option<String> {
        self.redirect_uris().into_iter().next()
    }

    pub fn check_redirect_uri(&self, redirect_uri: &str) -> bool {
        self.redirect_uris().iter().any(|u| u == redirect_uri)
    }

    pub fn check_client_secret(&self, client_secret: &str) -> bool {
        self.client_secret == client_secret
    }

    pub fn check_response_type(&self, response_type: &str) -> bool {
        self.response_types().iter().any(|t| t == response_type)
    }

    pub fn check_grant_type(&self, grant_type: &str) -> bool {
        self.grant_types().iter().any(|t| t == grant_type)
    }

    /// Scopes from `scope` that the client allows, in requested order.
    pub fn allowed_scope(&self, scope: &str) -> String {
        if scope.is_empty() {
            return String::new();
        }
        let allowed: Vec<&str> = self.scope.split_whitespace().collect();
        scope
            .split_whitespace()
            .filter(|s| allowed.contains(s))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCode {
    pub id: i64,
    pub user_id: Option<i64>,
    pub client_id: String,
    pub code: String,
    pub redirect_uri: String,
    pub scope: String,
    pub auth_time: i64,
}

impl AuthCode {
    pub fn is_expired(&self, now: i64) -> bool {
        self.auth_time + 300 < now
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub id: i64,
    pub user_id: Option<i64>,
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub scope: String,
    pub revoked: bool,
    pub issued_at: i64,
    pub expires_in: i64,
}

impl Token {
    pub fn is_expired(&self, now: i64) -> bool {
        self.issued_at + self.expires_in < now
    }

    /// A refresh token has no lifetime of its own: it works until the
    /// token is revoked (unlinked), however long ago its access token
    /// expired.
    pub fn is_refresh_token_active(&self) -> bool {
        !self.revoked && self.refresh_token.is_some()
    }
}

/// Drop records that can never be used again, so the file does not grow
/// with every login: revoked tokens, tokens whose access token expired
/// and that carry no usable refresh token, and expired authorization
/// codes. The in-memory state is pruned on load and before every
/// mutation; the file catches up on the next persist.
fn prune(data: &mut Data, now: i64) {
    data.tokens
        .retain(|t| !t.revoked && (!t.is_expired(now) || t.is_refresh_token_active()));
    data.codes.retain(|c| !c.is_expired(now));
}

/// Everything the file holds; the settings values are base64 (cookie_key
/// is 32 raw bytes).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Data {
    version: u32,
    settings: Vec<(String, String)>,
    users: Vec<User>,
    clients: Vec<Client>,
    codes: Vec<AuthCode>,
    tokens: Vec<Token>,
}

const FORMAT_VERSION: u32 = 1;

struct Store {
    data: Data,
    /// None for the in-memory database (--db :memory:).
    path: Option<PathBuf>,
}

impl Store {
    /// Persist atomically and durably: temp file in the same directory
    /// (owner-only mode — the file holds credentials), fsync, rename, and
    /// fsync of the directory so the rename survives power loss.
    fn persist(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let serialized =
            serde_json::to_vec_pretty(&self.data).context("cannot serialize database")?;
        let tmp = path.with_extension("tmp");
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            // never reuse a leftover tmp (it may have wider permissions or
            // be a symlink): O_EXCL after removal guarantees a fresh 0600
            // regular file
            let _ = fs::remove_file(&tmp);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)
                .with_context(|| format!("cannot create {}", tmp.display()))?;
            file.write_all(&serialized)
                .and_then(|_| file.sync_all())
                .with_context(|| format!("cannot write {}", tmp.display()))?;
        }
        fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
        // a bare filename ("db.json") has Some("") as its parent: that is
        // the current directory
        let dir_path = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => std::path::Path::new("."),
        };
        if let Ok(dir) = fs::File::open(dir_path) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

impl Store {
    /// Apply a change and persist it as one unit: on any failure —
    /// the change itself or the write — the in-memory state is rolled
    /// back, so memory never drifts from the file (a full or read-only
    /// flash must not turn retries into an ever-growing in-memory list).
    fn mutate<R>(&mut self, change: impl FnOnce(&mut Data) -> Result<R>) -> Result<R> {
        let snapshot = self.data.clone();
        let result = match change(&mut self.data) {
            Ok(result) => result,
            Err(e) => {
                self.data = snapshot;
                return Err(e);
            }
        };
        if let Err(e) = self.persist() {
            self.data = snapshot;
            return Err(e);
        }
        Ok(result)
    }
}

/// Unredeemed authorization codes kept per store, oldest evicted first;
/// a client only ever redeems its newest code.
const MAX_CODES: usize = 16;

/// What a newly issued token replaces within its (client, user) link.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Retire {
    /// Nothing (test fixtures).
    Nothing,
    /// The whole previous link: a new code grant supersedes it.
    Link,
    /// The previously refreshed access token: a refresh replaces it and
    /// leaves the root pair alone.
    Refreshed,
}

/// The fields of a token to store.
pub struct NewToken<'a> {
    pub client_id: &'a str,
    pub user_id: Option<i64>,
    pub access_token: &'a str,
    pub refresh_token: Option<&'a str>,
    pub scope: Option<&'a str>,
    pub issued_at: i64,
    pub expires_in: i64,
}

fn next_id<T>(items: &[T], id_of: impl Fn(&T) -> i64) -> i64 {
    items.iter().map(id_of).max().unwrap_or(0) + 1
}

#[derive(Clone)]
pub struct Db(Arc<Mutex<Store>>);

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        if path == ":memory:" {
            return Ok(Db(Arc::new(Mutex::new(Store {
                data: Data {
                    version: FORMAT_VERSION,
                    ..Data::default()
                },
                path: None,
            }))));
        }

        let path_buf = PathBuf::from(path);
        let data = match fs::read(&path_buf) {
            Ok(raw) => {
                if raw.starts_with(b"SQLite format 3\0") {
                    bail!(
                        "{path} is a sqlite database; convert it once with \
                         scripts/migrate-db.py first"
                    );
                }
                let mut data: Data = serde_json::from_slice(&raw)
                    .with_context(|| format!("cannot parse database {path}"))?;
                if data.version > FORMAT_VERSION {
                    bail!("database {path} has unsupported version {}", data.version);
                }
                // a missing version field deserializes as 0; real older
                // versions must go through an explicit migration
                if data.version == 0 {
                    data.version = FORMAT_VERSION;
                } else if data.version < FORMAT_VERSION {
                    bail!(
                        "database {path} has version {}: migrate it first",
                        data.version
                    );
                }
                prune(&mut data, crate::oauth::now());
                data
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data {
                version: FORMAT_VERSION,
                ..Data::default()
            },
            Err(e) => return Err(e).with_context(|| format!("cannot open database {path}")),
        };

        Ok(Db(Arc::new(Mutex::new(Store {
            data,
            path: Some(path_buf),
        }))))
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn setting(&self, option: &str) -> Result<Option<Vec<u8>>> {
        let store = self.store();
        for (name, value) in &store.data.settings {
            if name == option {
                return Ok(Some(
                    BASE64
                        .decode(value)
                        .with_context(|| format!("corrupt setting {option}"))?,
                ));
            }
        }
        Ok(None)
    }

    pub fn set_setting(&self, option: &str, value: &[u8]) -> Result<()> {
        let encoded = BASE64.encode(value);
        self.store().mutate(|data| {
            if let Some(entry) = data.settings.iter_mut().find(|(name, _)| name == option) {
                entry.1 = encoded;
            } else {
                data.settings.push((option.to_string(), encoded));
            }
            Ok(())
        })
    }

    pub fn user_by_id(&self, id: i64) -> Result<Option<User>> {
        Ok(self.store().data.users.iter().find(|u| u.id == id).cloned())
    }

    pub fn user_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(self
            .store()
            .data
            .users
            .iter()
            .find(|u| u.username == username)
            .cloned())
    }

    pub fn user_by_credentials(&self, username: &str, password: &str) -> Result<Option<User>> {
        Ok(self
            .store()
            .data
            .users
            .iter()
            .find(|u| u.username == username && u.password == password)
            .cloned())
    }

    pub fn insert_user(&self, username: &str, password: &str) -> Result<i64> {
        self.store().mutate(|data| {
            if data.users.iter().any(|u| u.username == username) {
                bail!("user {username} already exists");
            }
            let id = next_id(&data.users, |u| u.id);
            data.users.push(User {
                id,
                username: username.to_string(),
                password: password.to_string(),
            });
            Ok(id)
        })
    }

    pub fn clients_all(&self) -> Result<Vec<Client>> {
        Ok(self.store().data.clients.clone())
    }

    pub fn client_by_client_id(&self, client_id: &str) -> Result<Option<Client>> {
        Ok(self
            .store()
            .data
            .clients
            .iter()
            .find(|c| c.client_id == client_id)
            .cloned())
    }

    pub fn insert_client(&self, client: &Client) -> Result<()> {
        self.store().mutate(|data| {
            let mut client = client.clone();
            client.id = next_id(&data.clients, |c| c.id);
            data.clients.push(client);
            Ok(())
        })
    }

    pub fn codes_by_user(&self, user_id: i64) -> Result<Vec<AuthCode>> {
        Ok(self
            .store()
            .data
            .codes
            .iter()
            .filter(|c| c.user_id == Some(user_id))
            .cloned()
            .collect())
    }

    pub fn insert_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: Option<&str>,
        scope: &str,
        user_id: i64,
        auth_time: i64,
    ) -> Result<()> {
        self.store().mutate(|data| {
            prune(data, auth_time);
            // a new code supersedes the client's previous one for this
            // user, and the store never holds more than MAX_CODES
            data.codes
                .retain(|c| !(c.client_id == client_id && c.user_id == Some(user_id)));
            while data.codes.len() >= MAX_CODES {
                let oldest = data
                    .codes
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, c)| c.auth_time)
                    .map(|(i, _)| i);
                match oldest {
                    Some(i) => {
                        data.codes.remove(i);
                    }
                    None => break,
                }
            }
            let id = next_id(&data.codes, |c| c.id);
            data.codes.push(AuthCode {
                id,
                user_id: Some(user_id),
                client_id: client_id.to_string(),
                code: code.to_string(),
                redirect_uri: redirect_uri.unwrap_or("").to_string(),
                scope: scope.to_string(),
                auth_time,
            });
            Ok(())
        })
    }

    pub fn code_by_code_client(&self, code: &str, client_id: &str) -> Result<Option<AuthCode>> {
        Ok(self
            .store()
            .data
            .codes
            .iter()
            .find(|c| c.code == code && c.client_id == client_id)
            .cloned())
    }

    pub fn tokens_by_user(&self, user_id: i64) -> Result<Vec<Token>> {
        Ok(self
            .store()
            .data
            .tokens
            .iter()
            .filter(|t| t.user_id == Some(user_id))
            .cloned()
            .collect())
    }

    pub fn token_by_access_token(&self, access_token: &str) -> Result<Option<Token>> {
        Ok(self
            .store()
            .data
            .tokens
            .iter()
            .find(|t| t.access_token == access_token)
            .cloned())
    }

    pub fn token_by_refresh_token(&self, refresh_token: &str) -> Result<Option<Token>> {
        Ok(self
            .store()
            .data
            .tokens
            .iter()
            .find(|t| t.refresh_token.as_deref() == Some(refresh_token))
            .cloned())
    }

    /// Lookup for RFC7009 revocation: not-revoked token of the client, by hint.
    pub fn token_for_revocation(
        &self,
        client_id: &str,
        token: &str,
        token_type_hint: Option<&str>,
    ) -> Result<Option<Token>> {
        let store = self.store();
        let candidates = || {
            store
                .data
                .tokens
                .iter()
                .filter(|t| t.client_id == client_id && !t.revoked)
        };
        let by_access = || candidates().find(|t| t.access_token == token).cloned();
        let by_refresh = || {
            candidates()
                .find(|t| t.refresh_token.as_deref() == Some(token))
                .cloned()
        };
        Ok(match token_type_hint {
            Some("access_token") => by_access(),
            Some("refresh_token") => by_refresh(),
            _ => by_access().or_else(by_refresh),
        })
    }

    /// Store a token, retire what it replaces within its link, and
    /// consume the authorization code it was issued for — one persist,
    /// all or nothing. A link is the (client, user) token set: the root
    /// pair from the code grant plus the access token refreshed from it.
    pub fn insert_token(
        &self,
        token: NewToken,
        retire: Retire,
        consume_code: Option<i64>,
    ) -> Result<()> {
        self.store().mutate(|data| {
            if data
                .tokens
                .iter()
                .any(|t| t.access_token == token.access_token)
            {
                bail!("access token already exists");
            }
            if let Some(code_id) = consume_code {
                data.codes.retain(|c| c.id != code_id);
            }
            if retire != Retire::Nothing {
                revoke_matching(
                    data,
                    token.client_id,
                    token.user_id,
                    Some(token.access_token),
                    retire == Retire::Refreshed,
                );
            }
            let id = next_id(&data.tokens, |t| t.id);
            data.tokens.push(Token {
                id,
                user_id: token.user_id,
                client_id: token.client_id.to_string(),
                access_token: token.access_token.to_string(),
                refresh_token: token.refresh_token.map(str::to_string),
                scope: token.scope.unwrap_or("").to_string(),
                revoked: false,
                issued_at: token.issued_at,
                expires_in: token.expires_in,
            });
            prune(data, token.issued_at);
            Ok(())
        })
    }

    /// Revoke every token of one account link — the (client, user) pair:
    /// unlinking retires the root pair and the refreshed token alike.
    pub fn revoke_link(&self, client_id: &str, user_id: Option<i64>) -> Result<()> {
        self.store().mutate(|data| {
            revoke_matching(data, client_id, user_id, None, false);
            prune(data, crate::oauth::now());
            Ok(())
        })
    }
}

/// Mark the tokens of a link revoked, except `keep_access_token`; with
/// `refreshed_only` the root pair (tokens carrying a refresh token) is
/// spared. Callers prune afterwards.
fn revoke_matching(
    data: &mut Data,
    client_id: &str,
    user_id: Option<i64>,
    keep_access_token: Option<&str>,
    refreshed_only: bool,
) {
    for token in data.tokens.iter_mut() {
        if token.client_id == client_id
            && token.user_id == user_id
            && Some(token.access_token.as_str()) != keep_access_token
            && (!refreshed_only || token.refresh_token.is_none())
        {
            token.revoked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_file() {
        let dir = std::env::temp_dir().join(format!("sorokdva-db-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("db.json");
        let path_str = path.to_str().unwrap();
        let _ = fs::remove_file(&path);

        let db = Db::open(path_str).unwrap();
        db.set_setting("cookie_key", &[7u8; 32]).unwrap();
        let user_id = db.insert_user("username", "password").unwrap();
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "xxx",
                refresh_token: Some("yyy"),
                scope: Some("smarthome"),
                issued_at: 1_700_000_000,
                expires_in: 600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();

        // a fresh handle sees the persisted state
        let db = Db::open(path_str).unwrap();
        assert_eq!(db.setting("cookie_key").unwrap().unwrap(), vec![7u8; 32]);
        assert_eq!(
            db.user_by_credentials("username", "password")
                .unwrap()
                .unwrap()
                .id,
            user_id
        );
        let token = db.token_by_access_token("xxx").unwrap().unwrap();
        assert_eq!(token.refresh_token.as_deref(), Some("yyy"));
        assert!(!token.revoked);
        db.revoke_link(&token.client_id, token.user_id).unwrap();

        // revoked tokens are pruned rather than kept around
        let db = Db::open(path_str).unwrap();
        assert!(db.token_by_access_token("xxx").unwrap().is_none());

        // duplicates are rejected like the sqlite UNIQUE constraints
        assert!(db.insert_user("username", "other").is_err());

        // pruning: an expired token without a refresh token goes, an
        // expired one that still carries a refresh token stays (it is the
        // account link's root), and expired codes go
        let now = crate::oauth::now();
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "dead",
                refresh_token: None,
                scope: None,
                issued_at: now - 1000,
                expires_in: 10,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "root",
                refresh_token: Some("root-refresh"),
                scope: None,
                issued_at: now - 1000,
                expires_in: 10,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        db.insert_code("old-code", "client", None, "", user_id, now - 1000)
            .unwrap();
        // the next mutation prunes the ones above
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "fresh",
                refresh_token: None,
                scope: None,
                issued_at: now,
                expires_in: 3600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        assert!(db.token_by_access_token("dead").unwrap().is_none());
        assert!(db.token_by_access_token("root").unwrap().is_some());
        assert!(db.token_by_access_token("fresh").unwrap().is_some());
        assert!(db
            .code_by_code_client("old-code", "client")
            .unwrap()
            .is_none());

        // revoke_link retires every token of the (client, user) pair but
        // the one to keep, other users' links untouched
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "new-root",
                refresh_token: Some("new-refresh"),
                scope: None,
                issued_at: now,
                expires_in: 3600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(99),
                access_token: "other-user",
                refresh_token: Some("other-refresh"),
                scope: None,
                issued_at: now,
                expires_in: 3600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        db.insert_token(
            NewToken {
                client_id: "client2",
                user_id: Some(user_id),
                access_token: "other-client",
                refresh_token: None,
                scope: None,
                issued_at: now,
                expires_in: 3600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
        db.insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(user_id),
                access_token: "newest-root",
                refresh_token: Some("newest-refresh"),
                scope: None,
                issued_at: now,
                expires_in: 3600,
            },
            Retire::Link,
            None,
        )
        .unwrap();
        assert!(db.token_by_access_token("root").unwrap().is_none());
        assert!(db.token_by_access_token("fresh").unwrap().is_none());
        assert!(db.token_by_access_token("new-root").unwrap().is_none());
        assert!(db.token_by_access_token("newest-root").unwrap().is_some());
        assert!(db.token_by_access_token("other-user").unwrap().is_some());
        assert!(db.token_by_access_token("other-client").unwrap().is_some());

        // Retire::Refreshed spares the root pair and replaces the refreshed one
        for access in ["r1", "r2"] {
            db.insert_token(
                NewToken {
                    client_id: "client",
                    user_id: Some(user_id),
                    access_token: access,
                    refresh_token: None,
                    scope: None,
                    issued_at: now,
                    expires_in: 3600,
                },
                Retire::Refreshed,
                None,
            )
            .unwrap();
        }
        assert!(db.token_by_access_token("newest-root").unwrap().is_some());
        assert!(db.token_by_access_token("r1").unwrap().is_none());
        assert!(db.token_by_access_token("r2").unwrap().is_some());

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    /// A failed write rolls the in-memory state back: retries must not
    /// pile up records that the file never sees.
    #[test]
    fn failed_persist_rolls_back() {
        let dir = std::env::temp_dir().join(format!("sorokdva-db-rollback-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("db.json");
        let _ = fs::remove_file(&path);
        let db = Db::open(path.to_str().unwrap()).unwrap();
        let user_id = db.insert_user("u", "p").unwrap();
        // the directory disappears: every persist fails from now on
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
        for i in 0..5 {
            let access = format!("t{i}");
            assert!(db
                .insert_token(
                    NewToken {
                        client_id: "client",
                        user_id: Some(user_id),
                        access_token: &access,
                        refresh_token: None,
                        scope: None,
                        issued_at: crate::oauth::now(),
                        expires_in: 60,
                    },
                    Retire::Nothing,
                    None,
                )
                .is_err());
        }
        assert!(db.tokens_by_user(user_id).unwrap().is_empty());
        assert!(db.insert_user("v", "p").is_err());
        assert!(db.user_by_username("v").unwrap().is_none());
    }

    /// Codes: a new one supersedes the client's previous one for the same
    /// user, and the store never holds more than MAX_CODES.
    #[test]
    fn codes_supersede_and_cap() {
        let db = Db::open(":memory:").unwrap();
        let now = crate::oauth::now();
        db.insert_code("first", "client", None, "", 1, now).unwrap();
        db.insert_code("second", "client", None, "", 1, now)
            .unwrap();
        assert!(db.code_by_code_client("first", "client").unwrap().is_none());
        assert!(db
            .code_by_code_client("second", "client")
            .unwrap()
            .is_some());
        for user in 2..(MAX_CODES as i64 + 20) {
            db.insert_code(&format!("c{user}"), "client", None, "", user, now + user)
                .unwrap();
        }
        let mut total = 0;
        for user in 1..(MAX_CODES as i64 + 20) {
            total += db.codes_by_user(user).unwrap().len();
        }
        assert_eq!(total, MAX_CODES);
        // the oldest went first
        assert!(db
            .code_by_code_client("second", "client")
            .unwrap()
            .is_none());
    }

    #[test]
    fn sqlite_file_is_refused() {
        let dir = std::env::temp_dir().join(format!("sorokdva-db-sqlite-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("db.sqlite");
        fs::write(&path, b"SQLite format 3\0garbage").unwrap();
        let err = Db::open(path.to_str().unwrap()).map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("migrate-db.py"), "{err}");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
