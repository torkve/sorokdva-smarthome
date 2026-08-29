//! The OAuth2 subsystem: authorization_code + refresh_token grants,
//! RFC7009 revocation, RFC6750 bearer resource protection. The observable
//! behaviour (including several non-standard quirks marked below) is a
//! frozen API contract the live account link depends on.

pub mod error;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::web::Response;
use http::{header, HeaderMap, StatusCode};
use log::warn;
use serde_json::{json, Map, Value};

use crate::db::{AuthCode, Client, Db, NewToken, Retire, Token, User};
pub use error::{add_params_to_uri, redirect_response, OAuthError};

pub const ACCESS_TOKEN_LENGTH: usize = 42;
pub const REFRESH_TOKEN_LENGTH: usize = 48;
pub const AUTHORIZATION_CODE_LENGTH: usize = 48;
/// Access-token lifetime for both grants: a year. Yandex refreshes the
/// link only when the token expires, and a short lifetime leaves no slack
/// for delays. Authorization codes themselves expire after 300s
/// (AuthCode::is_expired).
pub const TOKEN_EXPIRES_IN: i64 = 60 * 60 * 24 * 365;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Random token over [a-zA-Z0-9].
pub fn generate_token(length: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut result = String::with_capacity(length);
    let mut buf = [0u8; 64];
    while result.len() < length {
        if getrandom::getrandom(&mut buf).is_err() {
            break;
        }
        for byte in buf {
            // rejection sampling for a uniform distribution
            if (byte & 0x3f) < 62 && result.len() < length {
                result.push(CHARS[(byte & 0x3f) as usize] as char);
            }
        }
    }
    result
}

/// The request parameters: `data` is query ∪ form (form wins on key
/// clashes), while `form` is the POST body alone — client credentials,
/// grant_type, code, refresh_token, token and token_type_hint are read
/// from the form only.
#[derive(Debug, Default, Clone)]
pub struct Params {
    data: HashMap<String, String>,
    form: HashMap<String, String>,
}

impl Params {
    pub fn from_query_and_form(query: &str, form: Option<&str>) -> Self {
        let mut data = HashMap::new();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            data.insert(key.into_owned(), value.into_owned());
        }
        let mut form_map = HashMap::new();
        if let Some(form) = form {
            for (key, value) in form_urlencoded::parse(form.as_bytes()) {
                data.insert(key.clone().into_owned(), value.clone().into_owned());
                form_map.insert(key.into_owned(), value.into_owned());
            }
        }
        Params {
            data,
            form: form_map,
        }
    }

    /// Merged query + form value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    /// POST body value only.
    pub fn form_get(&self, key: &str) -> Option<&str> {
        self.form.get(key).map(String::as_str)
    }

    pub fn state(&self) -> Option<String> {
        self.get("state").map(str::to_string)
    }
}

/// Success JSON response with the no-store/no-cache headers.
pub fn oauth_json_response(status: StatusCode, body: &Value) -> Response {
    http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::PRAGMA, "no-cache")
        .body(http_body_util::Full::new(bytes::Bytes::from(
            serde_json::to_string(body).unwrap_or_default(),
        )))
        .unwrap_or_else(|_| Response::new(http_body_util::Full::new(bytes::Bytes::new())))
}

pub struct ConsentGrant {
    pub client: Client,
    pub scope: Option<String>,
}

#[derive(Clone)]
pub struct OauthService {
    pub db: Db,
}

impl OauthService {
    /// Validate client_id / redirect_uri / response_type of an
    /// authorization request; returns (client, redirect_uri).
    fn validate_authorization_request(
        &self,
        params: &Params,
    ) -> Result<(Client, String), OAuthError> {
        let state = params.state();

        let Some(client_id) = params.get("client_id") else {
            return Err(OAuthError::InvalidClient { state });
        };
        let Ok(Some(client)) = self.db.client_by_client_id(client_id) else {
            return Err(OAuthError::InvalidClient { state });
        };

        let redirect_uri = match params.get("redirect_uri") {
            Some(redirect_uri) if !redirect_uri.is_empty() => {
                if !client.check_redirect_uri(redirect_uri) {
                    return Err(OAuthError::InvalidRequest {
                        description: Some(format!(
                            "Redirect URI {redirect_uri} is not supported by client."
                        )),
                        state,
                    });
                }
                redirect_uri.to_string()
            }
            _ => match client.default_redirect_uri() {
                Some(redirect_uri) => redirect_uri,
                None => {
                    return Err(OAuthError::InvalidRequest {
                        description: Some("Missing \"redirect_uri\" in request.".to_string()),
                        state,
                    });
                }
            },
        };

        let response_type = params.get("response_type").unwrap_or("");
        if !client.check_response_type(response_type) {
            return Err(OAuthError::UnauthorizedClient {
                description: Some(format!(
                    "The client is not authorized to use \"response_type={response_type}\""
                )),
                state,
                redirect_uri: Some(redirect_uri),
            });
        }

        // requested scopes are deliberately not validated: any scope
        // string is accepted as-is
        Ok((client, redirect_uri))
    }

    /// get_consent_grant for GET /oauth/authorize.
    pub fn get_consent_grant(&self, params: &Params) -> Result<ConsentGrant, OAuthError> {
        // only response_type=code is supported
        if params.get("response_type") != Some("code") {
            return Err(OAuthError::UnsupportedResponseType);
        }
        let (client, _redirect_uri) = self.validate_authorization_request(params)?;
        Ok(ConsentGrant {
            client,
            scope: params.get("scope").map(str::to_string),
        })
    }

    /// POST /oauth/authorize. The returned Response covers both the
    /// success redirect and error responses; Err(Uncaught) maps to a
    /// plain 500.
    pub fn create_authorization_response(
        &self,
        params: &Params,
        grant_user: Option<&User>,
    ) -> Result<Response, OAuthError> {
        // an unsupported response_type on POST gets a plain 500, not an
        // OAuth error body (frozen contract; GET answers 400 instead)
        if params.get("response_type") != Some("code") {
            return Err(OAuthError::Uncaught);
        }

        let (client, redirect_uri) = match self.validate_authorization_request(params) {
            Ok(found) => found,
            Err(error) => return Ok(error.into_response()),
        };

        let Some(user) = grant_user else {
            return Ok(OAuthError::AccessDenied {
                state: params.state(),
                redirect_uri,
            }
            .into_response());
        };

        let code = generate_token(AUTHORIZATION_CODE_LENGTH);
        if self
            .db
            .insert_code(
                &code,
                &client.client_id,
                params.get("redirect_uri"),
                params.get("scope").unwrap_or(""),
                user.id,
                now(),
            )
            .is_err()
        {
            return Err(OAuthError::Uncaught);
        }

        let mut redirect_params = vec![("code".to_string(), code)];
        if let Some(state) = params.state() {
            redirect_params.push(("state".to_string(), state));
        }
        let location = add_params_to_uri(&redirect_uri, &redirect_params);
        Ok(redirect_response(&location))
    }

    /// Client authentication with the only registered method,
    /// client_secret_post.
    fn authenticate_client(&self, params: &Params) -> Result<Client, OAuthError> {
        let state = params.state();
        let (Some(client_id), Some(client_secret)) = (
            params.form_get("client_id"),
            params.form_get("client_secret"),
        ) else {
            warn!(target: "oauth", "client auth: client_id/client_secret missing from the form body");
            return Err(OAuthError::InvalidClient { state });
        };
        if client_id.is_empty() || client_secret.is_empty() {
            warn!(target: "oauth", "client auth: empty client_id or client_secret");
            return Err(OAuthError::InvalidClient { state });
        }
        let Ok(Some(client)) = self.db.client_by_client_id(client_id) else {
            warn!(target: "oauth", "client auth: unknown client_id {client_id:?}");
            return Err(OAuthError::InvalidClient { state });
        };
        if !client.check_client_secret(client_secret) {
            warn!(target: "oauth", "client auth: wrong secret for client_id {client_id}");
            return Err(OAuthError::InvalidClient { state });
        }
        if client.token_endpoint_auth_method != "client_secret_post" {
            warn!(
                target: "oauth",
                "client auth: client_id {client_id} has auth method {:?}, need client_secret_post",
                client.token_endpoint_auth_method
            );
            return Err(OAuthError::InvalidClient { state });
        }
        Ok(client)
    }

    /// Generate, store and serialize a bearer token (optionally with a
    /// refresh token).
    fn issue_token(
        &self,
        client: &Client,
        user_id: Option<i64>,
        scope: &str,
        include_refresh_token: bool,
        retire: Retire,
        consume_code: Option<i64>,
    ) -> Result<(Value, String), OAuthError> {
        let scope = client.allowed_scope(scope);
        let access_token = generate_token(ACCESS_TOKEN_LENGTH);

        let mut token = Map::new();
        token.insert("token_type".into(), json!("Bearer"));
        token.insert("access_token".into(), json!(access_token));
        token.insert("expires_in".into(), json!(TOKEN_EXPIRES_IN));
        let refresh_token = if include_refresh_token {
            let refresh_token = generate_token(REFRESH_TOKEN_LENGTH);
            token.insert("refresh_token".into(), json!(refresh_token));
            Some(refresh_token)
        } else {
            None
        };
        if !scope.is_empty() {
            token.insert("scope".into(), json!(scope));
        }

        self.db
            .insert_token(
                NewToken {
                    client_id: &client.client_id,
                    user_id,
                    access_token: &access_token,
                    refresh_token: refresh_token.as_deref(),
                    scope: if scope.is_empty() { None } else { Some(&scope) },
                    issued_at: now(),
                    expires_in: TOKEN_EXPIRES_IN,
                },
                retire,
                consume_code,
            )
            .map_err(|e| {
                warn!(target: "oauth", "cannot store the issued token: {e}");
                OAuthError::Uncaught
            })?;

        Ok((Value::Object(token), access_token))
    }

    fn token_response_authorization_code(&self, params: &Params) -> Result<Response, OAuthError> {
        let client = self.authenticate_client(params)?;

        if !client.check_grant_type("authorization_code") {
            return Err(OAuthError::UnauthorizedClient {
                description: Some(
                    "The client is not authorized to use \"grant_type=authorization_code\""
                        .to_string(),
                ),
                state: None,
                redirect_uri: None,
            });
        }

        let Some(code) = params.form_get("code") else {
            return Err(OAuthError::InvalidRequest {
                description: Some("Missing \"code\" in request.".to_string()),
                state: None,
            });
        };

        let auth_code: AuthCode = match self.db.code_by_code_client(code, &client.client_id) {
            Ok(Some(auth_code)) if !auth_code.is_expired(now()) => auth_code,
            Ok(Some(auth_code)) => {
                warn!(
                    target: "oauth",
                    "code grant: code #{} for client {} expired (auth_time={}, now={})",
                    auth_code.id, client.client_id, auth_code.auth_time, now()
                );
                return Err(OAuthError::InvalidGrant {
                    description: Some("Invalid \"code\" in request.".to_string()),
                });
            }
            Ok(None) => {
                warn!(target: "oauth", "code grant: no such code for client {}", client.client_id);
                return Err(OAuthError::InvalidGrant {
                    description: Some("Invalid \"code\" in request.".to_string()),
                });
            }
            Err(e) => {
                warn!(target: "oauth", "code grant: code lookup failed: {e}");
                return Err(OAuthError::InvalidGrant {
                    description: Some("Invalid \"code\" in request.".to_string()),
                });
            }
        };

        if !auth_code.redirect_uri.is_empty()
            && params.get("redirect_uri") != Some(auth_code.redirect_uri.as_str())
        {
            warn!(
                target: "oauth",
                "code grant: redirect_uri {:?} does not match the code's {:?}",
                params.get("redirect_uri"), auth_code.redirect_uri
            );
            return Err(OAuthError::InvalidGrant {
                description: Some("Invalid \"redirect_uri\" in request.".to_string()),
            });
        }

        let user = match auth_code
            .user_id
            .and_then(|id| self.db.user_by_id(id).ok().flatten())
        {
            Some(user) => user,
            None => {
                return Err(OAuthError::InvalidGrant {
                    description: Some("There is no \"user\" for this code.".to_string()),
                });
            }
        };

        // a new link supersedes the previous one and consumes its code —
        // stored as one unit, so the store holds exactly one live link per
        // (client, user) whatever happens
        let (token, _) = self.issue_token(
            &client,
            Some(user.id),
            &auth_code.scope,
            client.check_grant_type("refresh_token"),
            Retire::Link,
            Some(auth_code.id),
        )?;
        Ok(oauth_json_response(StatusCode::OK, &token))
    }

    fn token_response_refresh_token(&self, params: &Params) -> Result<Response, OAuthError> {
        let client = self.authenticate_client(params)?;

        if !client.check_grant_type("refresh_token") {
            return Err(OAuthError::UnauthorizedClient {
                description: None,
                state: None,
                redirect_uri: None,
            });
        }

        let Some(refresh_token) = params.form_get("refresh_token") else {
            return Err(OAuthError::InvalidRequest {
                description: Some("Missing \"refresh_token\" in request.".to_string()),
                state: None,
            });
        };

        let token: Token = match self.db.token_by_refresh_token(refresh_token) {
            Ok(Some(token))
                if token.is_refresh_token_active() && token.client_id == client.client_id =>
            {
                token
            }
            Ok(Some(token)) => {
                let reason = if token.client_id != client.client_id {
                    "belongs to another client"
                } else {
                    "is revoked"
                };
                warn!(
                    target: "oauth",
                    "refresh grant: token #{} for client {} {reason} \
                     (issued_at={}, expires_in={}, now={})",
                    token.id, client.client_id, token.issued_at, token.expires_in, now()
                );
                return Err(OAuthError::InvalidGrant { description: None });
            }
            Ok(None) => {
                warn!(target: "oauth", "refresh grant: unknown refresh token for client {}", client.client_id);
                return Err(OAuthError::InvalidGrant { description: None });
            }
            Err(e) => {
                warn!(target: "oauth", "refresh grant: token lookup failed: {e}");
                return Err(OAuthError::InvalidGrant { description: None });
            }
        };

        // optional scope narrowing must stay within the original scope
        let requested_scope = params.get("scope").unwrap_or("");
        if !requested_scope.is_empty() {
            if token.scope.is_empty() {
                return Err(OAuthError::InvalidScope);
            }
            let original: Vec<&str> = token.scope.split_whitespace().collect();
            if !requested_scope
                .split_whitespace()
                .all(|s| original.contains(&s))
            {
                return Err(OAuthError::InvalidScope);
            }
        }

        let user = match token
            .user_id
            .and_then(|id| self.db.user_by_id(id).ok().flatten())
        {
            Some(user) => user,
            None => {
                return Err(OAuthError::InvalidRequest {
                    description: Some("There is no \"user\" for this token.".to_string()),
                    state: None,
                });
            }
        };

        let scope = if requested_scope.is_empty() {
            token.scope.clone()
        } else {
            requested_scope.to_string()
        };
        // No refresh-token rotation: the response carries only a new access
        // token and the client keeps using the refresh token it has, which
        // stays valid until revoked (unlink). The previously refreshed
        // access token is retired in the same write, so a link never holds
        // more than the root pair plus one refreshed token.
        let (new_token, _) = self.issue_token(
            &client,
            Some(user.id),
            &scope,
            false,
            Retire::Refreshed,
            None,
        )?;
        Ok(oauth_json_response(StatusCode::OK, &new_token))
    }

    /// GET+POST /oauth/token. Unsupported grant types and non-POST
    /// methods get a plain 500, not an OAuth error body (frozen
    /// contract).
    pub fn create_token_response(&self, method: &str, params: &Params) -> Response {
        let grant_type = params.form_get("grant_type").unwrap_or("");
        let client_id = params.form_get("client_id").unwrap_or("");
        if method != "POST" {
            warn!(target: "oauth", "token request: method {method} is not POST -> 500");
            return internal_error_response();
        }
        let result = match grant_type {
            "authorization_code" => self.token_response_authorization_code(params),
            "refresh_token" => self.token_response_refresh_token(params),
            _ => {
                warn!(
                    target: "oauth",
                    "token request: unsupported grant_type {grant_type:?} from client {client_id:?} -> 500"
                );
                Err(OAuthError::Uncaught)
            }
        };
        match result {
            Ok(response) => response,
            Err(OAuthError::Uncaught) => internal_error_response(),
            Err(error) => {
                warn!(
                    target: "oauth",
                    "token request: grant_type={grant_type:?} client_id={client_id:?} -> {} {}",
                    error.status(),
                    error.error_code()
                );
                error.into_response()
            }
        }
    }

    /// The RFC7009 revocation endpoint, POST /oauth/revoke.
    pub fn create_revocation_response(&self, params: &Params) -> Response {
        match self.revoke(params) {
            Ok(()) => oauth_json_response(StatusCode::OK, &Value::Object(Map::new())),
            Err(OAuthError::Uncaught) => internal_error_response(),
            Err(error) => error.into_response(),
        }
    }

    fn revoke(&self, params: &Params) -> Result<(), OAuthError> {
        let client = self.authenticate_client(params)?;

        let Some(token) = params.form_get("token") else {
            return Err(OAuthError::InvalidRequest {
                description: None,
                state: None,
            });
        };

        let hint = params.form_get("token_type_hint");
        if let Some(hint) = hint {
            if !hint.is_empty() && hint != "access_token" && hint != "refresh_token" {
                return Err(OAuthError::UnsupportedTokenType {
                    with_auth_header: false,
                });
            }
        }

        // revoking any token of a link (Yandex presents the access token it
        // holds, which after a refresh is not the root pair) retires the
        // whole link, refresh token included
        if let Ok(Some(found)) = self.db.token_for_revocation(&client.client_id, token, hint) {
            self.unlink(&found);
        }
        Ok(())
    }

    /// Retire the whole link a token belongs to. A failed persist is
    /// logged: the in-memory state is already unlinked, but the file would
    /// bring the link back after a restart.
    pub fn unlink(&self, token: &Token) {
        if let Err(e) = self.db.revoke_link(&token.client_id, token.user_id) {
            warn!(target: "oauth", "cannot persist the unlink of token #{}: {e}", token.id);
        }
    }

    /// The RFC6750 resource protector.
    pub fn acquire_token(
        &self,
        headers: &HeaderMap,
        required_scope: &str,
    ) -> Result<Token, OAuthError> {
        let Some(auth) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        else {
            return Err(OAuthError::MissingAuthorization);
        };
        if auth.is_empty() {
            return Err(OAuthError::MissingAuthorization);
        }

        // split on the first whitespace run (any amount, any kind)
        let trimmed = auth.trim_start();
        let (token_type, token_string) = match trimmed.find(char::is_whitespace) {
            Some(pos) => {
                let (first, rest) = trimmed.split_at(pos);
                let rest = rest.trim_start();
                if rest.is_empty() {
                    return Err(OAuthError::UnsupportedTokenType {
                        with_auth_header: true,
                    });
                }
                (first, rest.trim_end())
            }
            None => {
                return Err(OAuthError::UnsupportedTokenType {
                    with_auth_header: true,
                });
            }
        };
        if token_type.to_lowercase() != "bearer" {
            return Err(OAuthError::UnsupportedTokenType {
                with_auth_header: true,
            });
        }

        let token = match self.db.token_by_access_token(token_string) {
            Ok(Some(token)) => token,
            _ => return Err(OAuthError::InvalidToken),
        };
        if token.is_expired(now()) || token.revoked {
            return Err(OAuthError::InvalidToken);
        }

        if !required_scope.is_empty() {
            let token_scopes: Vec<&str> = token.scope.split_whitespace().collect();
            let insufficient = token_scopes.is_empty()
                || !required_scope
                    .split_whitespace()
                    .all(|s| token_scopes.contains(&s));
            if insufficient {
                return Err(OAuthError::InsufficientScope);
            }
        }

        Ok(token)
    }
}

/// The plain-text 500 response used for uncaught error conditions.
pub fn internal_error_response() -> Response {
    http::Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(http_body_util::Full::new(bytes::Bytes::from(
            "500 Internal Server Error\n\nServer got itself in trouble",
        )))
        .unwrap_or_else(|_| Response::new(http_body_util::Full::new(bytes::Bytes::new())))
}
