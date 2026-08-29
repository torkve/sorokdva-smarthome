//! OAuth2 error responses. Body parameter order, redirect encoding and
//! WWW-Authenticate headers are part of the frozen wire format the
//! account link depends on -- do not "clean up" oddities here.

use crate::web::Response;
use http::{header, HeaderValue, StatusCode};

const INVALID_TOKEN_DESCRIPTION: &str = "The access token provided is expired, revoked, \
                                         malformed, or invalid for other reasons.";

#[derive(Debug, Clone)]
pub enum OAuthError {
    InvalidRequest {
        description: Option<String>,
        state: Option<String>,
    },
    InvalidClient {
        state: Option<String>,
    },
    InvalidGrant {
        description: Option<String>,
    },
    UnauthorizedClient {
        description: Option<String>,
        state: Option<String>,
        redirect_uri: Option<String>,
    },
    UnsupportedResponseType,
    InvalidScope,
    AccessDenied {
        state: Option<String>,
        redirect_uri: String,
    },
    /// rfc6750 bearer errors (resource protector)
    MissingAuthorization,
    UnsupportedTokenType {
        /// raised with auth_type "bearer" by the protector,
        /// without it by the revocation endpoint
        with_auth_header: bool,
    },
    InvalidToken,
    InsufficientScope,
    /// A request condition (unsupported grant/response type, and the like)
    /// that the API contract answers with a plain 500, not an OAuth error
    /// body.
    Uncaught,
}

impl OAuthError {
    pub fn error_code(&self) -> &'static str {
        match self {
            OAuthError::InvalidRequest { .. } => "invalid_request",
            OAuthError::InvalidClient { .. } => "invalid_client",
            OAuthError::InvalidGrant { .. } => "invalid_grant",
            OAuthError::UnauthorizedClient { .. } => "unauthorized_client",
            OAuthError::UnsupportedResponseType => "unsupported_response_type",
            OAuthError::InvalidScope => "invalid_scope",
            OAuthError::AccessDenied { .. } => "access_denied",
            OAuthError::MissingAuthorization => "missing_authorization",
            OAuthError::UnsupportedTokenType { .. } => "unsupported_token_type",
            OAuthError::InvalidToken => "invalid_token",
            OAuthError::InsufficientScope => "insufficient_scope",
            OAuthError::Uncaught => "internal",
        }
    }

    fn description(&self) -> Option<String> {
        match self {
            OAuthError::InvalidRequest { description, .. } => description.clone(),
            OAuthError::InvalidGrant { description } => description.clone(),
            OAuthError::UnauthorizedClient { description, .. } => description.clone(),
            OAuthError::InvalidScope => {
                Some("The requested scope is invalid, unknown, or malformed.".to_string())
            }
            OAuthError::AccessDenied { .. } => {
                Some("The resource owner or authorization server denied the request".to_string())
            }
            OAuthError::MissingAuthorization => {
                Some("Missing \"Authorization\" in headers.".to_string())
            }
            OAuthError::InvalidToken => Some(INVALID_TOKEN_DESCRIPTION.to_string()),
            OAuthError::InsufficientScope => Some(
                "The request requires higher privileges than provided by the access token."
                    .to_string(),
            ),
            _ => None,
        }
    }

    fn state(&self) -> Option<&str> {
        match self {
            OAuthError::InvalidRequest { state, .. }
            | OAuthError::InvalidClient { state }
            | OAuthError::UnauthorizedClient { state, .. }
            | OAuthError::AccessDenied { state, .. } => state.as_deref(),
            _ => None,
        }
    }

    fn redirect_uri(&self) -> Option<&str> {
        match self {
            OAuthError::UnauthorizedClient { redirect_uri, .. } => redirect_uri.as_deref(),
            OAuthError::AccessDenied { redirect_uri, .. } => Some(redirect_uri.as_str()),
            _ => None,
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            OAuthError::MissingAuthorization
            | OAuthError::UnsupportedTokenType { .. }
            | OAuthError::InvalidToken => StatusCode::UNAUTHORIZED,
            OAuthError::InsufficientScope => StatusCode::FORBIDDEN,
            OAuthError::Uncaught => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        }
    }

    /// The ordered body parameters: error, error_description?, state?.
    pub fn body_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = vec![("error".to_string(), self.error_code().to_string())];
        if let Some(description) = self.description() {
            pairs.push(("error_description".to_string(), description));
        }
        if let Some(state) = self.state() {
            pairs.push(("state".to_string(), state.to_string()));
        }
        pairs
    }

    fn www_authenticate(&self) -> Option<String> {
        match self {
            OAuthError::MissingAuthorization => Some(
                "bearer error=\"missing_authorization\", \
                 error_description=\"Missing \"Authorization\" in headers.\""
                    .to_string(),
            ),
            OAuthError::UnsupportedTokenType {
                with_auth_header: true,
            } => {
                Some("bearer error=\"unsupported_token_type\", error_description=\"\"".to_string())
            }
            // the literal extra_attributes="{}" artifact is part of the
            // frozen header format; keep it byte-identical
            OAuthError::InvalidToken => Some(format!(
                "Bearer extra_attributes=\"{{}}\", error=\"invalid_token\", \
                 error_description=\"{INVALID_TOKEN_DESCRIPTION}\""
            )),
            _ => None,
        }
    }

    /// 302 redirect carrying the error params when redirect_uri is set,
    /// JSON body otherwise.
    pub fn into_response(self) -> Response {
        if let Some(redirect_uri) = self.redirect_uri() {
            let location = add_params_to_uri(redirect_uri, &self.body_pairs());
            return redirect_response(&location);
        }
        self.into_json_response(None)
    }

    /// JSON error response; `force_status` overrides the error's own status
    /// (GET /oauth/authorize converts every error to HTTP 400).
    pub fn into_json_response(self, force_status: Option<StatusCode>) -> Response {
        let status = force_status.unwrap_or_else(|| self.status());
        // serde_json::Map sorts keys; the frozen body order
        // (error, error_description, state) happens to be alphabetical.
        // Adding a param that breaks that coincidence needs a serializer
        // honouring body_pairs() order instead.
        let mut map = serde_json::Map::new();
        for (key, value) in self.body_pairs() {
            map.insert(key, serde_json::Value::String(value));
        }
        let body = serde_json::to_string(&map).unwrap_or_default();
        let mut builder = http::Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::PRAGMA, "no-cache");
        if let Some(authenticate) = self.www_authenticate() {
            if let Ok(value) = HeaderValue::from_str(&authenticate) {
                builder = builder.header(header::WWW_AUTHENTICATE, value);
            }
        }
        builder
            .body(http_body_util::Full::new(bytes::Bytes::from(body)))
            .unwrap_or_else(|_| Response::new(http_body_util::Full::new(bytes::Bytes::new())))
    }
}

/// Append the params to the URI's query string, urlencoded with
/// quote_plus semantics (spaces become '+').
pub fn add_params_to_uri(uri: &str, params: &[(String, String)]) -> String {
    let mut encoded = form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        encoded.append_pair(key, value);
    }
    let encoded = encoded.finish();
    if uri.contains('?') {
        format!("{uri}&{encoded}")
    } else {
        format!("{uri}?{encoded}")
    }
}

/// 302 with an empty text/plain body (the content type is set even
/// though the body is empty).
pub fn redirect_response(location: &str) -> Response {
    let mut builder = http::Response::builder()
        .status(StatusCode::FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
    if let Ok(value) = HeaderValue::from_str(location) {
        builder = builder.header(header::LOCATION, value);
    }
    builder
        .body(http_body_util::Full::new(bytes::Bytes::new()))
        .unwrap_or_else(|_| Response::new(http_body_util::Full::new(bytes::Bytes::new())))
}
