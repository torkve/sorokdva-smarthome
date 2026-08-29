pub mod templates;

use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::Arc;

use bytes::Bytes;
use http::{header, HeaderMap, Method, StatusCode, Uri};
use http_body_util::{BodyExt, Full, Limited};
use hyper_util::rt::TokioIo;
use log::{debug, info, log_enabled, Level};
use serde_json::{json, Value};

use crate::db::{Client, Db, User};
use crate::devices::{self, BuiltDevices};
use crate::oauth::{generate_token, internal_error_response, OauthService, Params};
use crate::session::{Session, SessionStore};

/// The response type used across the server (a fully buffered body).
pub type Response = http::Response<Full<Bytes>>;

/// Maximum accepted request body size; larger bodies get a 413.
const MAX_BODY: usize = 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub oauth: OauthService,
    pub sessions: SessionStore,
    pub devices: Arc<BuiltDevices>,
    /// Normalized subpath prefix: "" or e.g. "/alice".
    pub prefix: String,
    pub debug: bool,
    pub proxy: bool,
}

impl AppState {
    fn url_for(&self, path: &str) -> String {
        format!("{}{}", self.prefix, path)
    }

    fn session_user(&self, headers: &HeaderMap) -> (Session, Option<User>) {
        let session = self.sessions.load(headers);
        let user = session
            .user_id()
            .and_then(|id| self.db.user_by_id(id).ok().flatten());
        (session, user)
    }

    /// The request's absolute URL, honouring X-Forwarded-* with --proxy.
    fn request_url(&self, headers: &HeaderMap, uri: &Uri) -> String {
        let forwarded = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let scheme = if self.proxy {
            forwarded("x-forwarded-proto").unwrap_or_else(|| "http".to_string())
        } else {
            "http".to_string()
        };
        let host = self
            .proxy
            .then(|| forwarded("x-forwarded-host"))
            .flatten()
            .or_else(|| {
                headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "localhost".to_string());
        let path_qs = uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or_else(|| uri.path());
        format!("{scheme}://{host}{path_qs}")
    }
}

/// One buffered request: everything the handlers need.
struct Ctx {
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
}

fn text_response(status: StatusCode, content_type: &str, body: &str) -> Response {
    http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap_or_else(|_| http::Response::new(Full::new(Bytes::new())))
}

fn plain(status: StatusCode, body: &str) -> Response {
    text_response(status, "text/plain; charset=utf-8", body)
}

fn html(body: String) -> Response {
    text_response(StatusCode::OK, "text/html; charset=utf-8", &body)
}

/// 200 JSON response.
fn json_response(value: &Value) -> Response {
    text_response(
        StatusCode::OK,
        "application/json; charset=utf-8",
        &serde_json::to_string(value).unwrap_or_default(),
    )
}

/// 302 redirect with a "302: Found" text body.
fn found(location: &str) -> Response {
    let mut response = plain(StatusCode::FOUND, "302: Found");
    if let Ok(value) = header::HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

fn with_cookie(mut response: Response, cookie: String) -> Response {
    if let Ok(value) = header::HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// The body is parsed as a form only for the exact mimetype
/// application/x-www-form-urlencoded (";"-cut, trimmed, lowercased);
/// a missing, empty or malformed Content-Type yields no form. Multipart
/// is not supported: no real client (Yandex, browsers posting these
/// forms) sends it.
fn is_form_content_type(headers: &HeaderMap) -> bool {
    let Some(raw) = headers.get(header::CONTENT_TYPE) else {
        return false;
    };
    let Ok(raw) = raw.to_str() else {
        return false;
    };
    let mimetype = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    mimetype == "application/x-www-form-urlencoded"
}

fn parse_form(headers: &HeaderMap, body: &[u8]) -> Params {
    let form = is_form_content_type(headers).then(|| String::from_utf8_lossy(body).into_owned());
    Params::from_query_and_form("", form.as_deref())
}

fn merged_params(uri: &Uri, headers: &HeaderMap, body: Option<&[u8]>) -> Params {
    let form = body
        .filter(|_| is_form_content_type(headers))
        .map(|b| String::from_utf8_lossy(b).into_owned());
    Params::from_query_and_form(uri.query().unwrap_or(""), form.as_deref())
}

// ------------------------------------------------------------------ auth pages

async fn auth_get(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let (_session, user) = state.session_user(&ctx.headers);
    let post_query = ctx.uri.query().unwrap_or("");

    let (clients, codes, tokens) = if let Some(user) = &user {
        let clients = state.db.clients_all().unwrap_or_default();
        let with_client = |client_id: &str| -> Option<Client> {
            state.db.client_by_client_id(client_id).ok().flatten()
        };
        let codes = state
            .db
            .codes_by_user(user.id)
            .unwrap_or_default()
            .into_iter()
            .map(|code| {
                let client = with_client(&code.client_id);
                (code, client)
            })
            .collect();
        let tokens = state
            .db
            .tokens_by_user(user.id)
            .unwrap_or_default()
            .into_iter()
            .map(|token| {
                let client = with_client(&token.client_id);
                (token, client)
            })
            .collect();
        (clients, codes, tokens)
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    html(templates::index_page(&templates::IndexContext {
        user: user.as_ref(),
        clients: &clients,
        codes: &codes,
        tokens: &tokens,
        post_query,
        debug: state.debug,
        prefix: &state.prefix,
    }))
}

async fn auth_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let form = parse_form(&ctx.headers, &ctx.body);
    let username = form.get("username").unwrap_or("");
    let password = form.get("password").unwrap_or("");

    let user = state
        .db
        .user_by_credentials(username, password)
        .ok()
        .flatten();
    match user {
        Some(user) => {
            let mut session = state.sessions.load(&ctx.headers);
            session.remember(user.id);
            let response = found(&state.request_url(&ctx.headers, &ctx.uri));
            with_cookie(response, state.sessions.save_cookie(&session))
        }
        None => text_response(
            StatusCode::FORBIDDEN,
            "application/octet-stream",
            "Who are you? Go away!",
        ),
    }
}

async fn logout_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let location = match ctx.uri.query() {
        Some(query) if !query.is_empty() => format!("{}?{}", state.url_for("/auth"), query),
        _ => state.url_for("/auth"),
    };
    let response = found(&location);
    // the clear cookie is sent only when there was an identity to
    // forget; an untouched session is not re-saved
    let mut session = state.sessions.load(&ctx.headers);
    if session.has_identity() {
        session.forget();
        with_cookie(response, state.sessions.clear_cookie())
    } else {
        response
    }
}

async fn register_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let form = parse_form(&ctx.headers, &ctx.body);
    let (Some(username), Some(password)) = (form.get("username"), form.get("password")) else {
        return internal_error_response();
    };

    if let Ok(Some(_)) = state.db.user_by_username(username) {
        return plain(StatusCode::BAD_REQUEST, "User already exists");
    }
    let Ok(user_id) = state.db.insert_user(username, password) else {
        return internal_error_response();
    };

    let mut session = state.sessions.load(&ctx.headers);
    session.remember(user_id);
    let response = found(&state.url_for("/auth"));
    with_cookie(response, state.sessions.save_cookie(&session))
}

async fn create_client_get(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let (_session, user) = state.session_user(&ctx.headers);
    if user.is_none() {
        return found(&state.url_for("/auth"));
    }
    html(templates::create_client_page(&state.prefix))
}

async fn create_client_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let (_session, user) = state.session_user(&ctx.headers);
    let Some(user) = user else {
        return plain(StatusCode::UNAUTHORIZED, "401: Unauthorized");
    };

    let form = parse_form(&ctx.headers, &ctx.body);
    let client = Client {
        user_id: Some(user.id),
        client_id: generate_token(24),
        client_secret: generate_token(48),
        token_endpoint_auth_method: "client_secret_post".to_string(),
        issued_at: crate::oauth::now(),
        expires_at: 0,
        client_name: form.get("client_name").unwrap_or("").to_string(),
        client_uri: form.get("client_uri").unwrap_or("").to_string(),
        scope: form.get("scope").unwrap_or("").to_string(),
        redirect_uri: form.get("redirect_uri").unwrap_or("").to_string(),
        grant_type: form.get("grant_type").unwrap_or("").to_string(),
        response_type: form.get("response_type").unwrap_or("").to_string(),
        ..Client::default()
    };
    if state.db.insert_client(&client).is_err() {
        return internal_error_response();
    }
    found(&state.url_for("/auth"))
}

// ------------------------------------------------------------------ oauth

async fn oauthorize_get(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let (_session, user) = state.session_user(&ctx.headers);
    if user.is_none() {
        return found(&state.url_for("/auth"));
    }

    let params = merged_params(&ctx.uri, &ctx.headers, None);
    match state.oauth.get_consent_grant(&params) {
        Ok(grant) => {
            // a missing scope renders as the literal string "None"
            let scope = grant.scope.unwrap_or_else(|| "None".to_string());
            html(templates::oauth_page(&grant.client.client_name, &scope))
        }
        Err(error) => {
            log::warn!(
                target: "oauth",
                "authorize (GET): client_id={:?} response_type={:?} -> 400 {}",
                params.get("client_id"),
                params.get("response_type"),
                error.error_code()
            );
            error.into_json_response(Some(StatusCode::BAD_REQUEST))
        }
    }
}

async fn oauthorize_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let (_session, user) = state.session_user(&ctx.headers);
    let Some(user) = user else {
        return plain(StatusCode::UNAUTHORIZED, "401: Unauthorized");
    };

    let params = merged_params(&ctx.uri, &ctx.headers, Some(&ctx.body));
    let confirmed = params
        .form_get("confirm")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let grant_user = confirmed.then_some(&user);

    match state
        .oauth
        .create_authorization_response(&params, grant_user)
    {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                target: "oauth",
                "authorize (POST): client_id={:?} response_type={:?} -> 500 {}",
                params.get("client_id"),
                params.get("response_type"),
                error.error_code()
            );
            internal_error_response()
        }
    }
}

async fn token_any(ctx: &Ctx) -> Response {
    let body = if ctx.method == Method::POST {
        Some(&ctx.body[..])
    } else {
        None
    };
    let params = merged_params(&ctx.uri, &ctx.headers, body);
    ctx.state
        .oauth
        .create_token_response(ctx.method.as_str(), &params)
}

async fn revoke_post(ctx: &Ctx) -> Response {
    let params = merged_params(&ctx.uri, &ctx.headers, Some(&ctx.body));
    ctx.state.oauth.create_revocation_response(&params)
}

async fn me_get(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let token = match state.oauth.acquire_token(&ctx.headers, "profile") {
        Ok(token) => token,
        Err(error) => return error.into_json_response(None),
    };
    let user = token
        .user_id
        .and_then(|id| state.db.user_by_id(id).ok().flatten());
    match user {
        Some(user) => json_response(&json!({"id": user.id, "username": user.username})),
        None => internal_error_response(),
    }
}

// ------------------------------------------------------------------ smarthome

fn ping() -> Response {
    plain(StatusCode::OK, "200: OK")
}

fn request_id_value(headers: &HeaderMap) -> Value {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|v| json!(v))
        .unwrap_or(Value::Null)
}

/// Yandex unlink: the request carries only the bearer token, so the
/// link to retire is the one that token belongs to.
async fn unlink_post(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let token = match state.oauth.acquire_token(&ctx.headers, "smarthome") {
        Ok(token) => token,
        Err(error) => return error.into_json_response(None),
    };
    state.oauth.unlink(&token);
    json_response(&json!({"request_id": request_id_value(&ctx.headers)}))
}

async fn list_devices(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    let token = match state.oauth.acquire_token(&ctx.headers, "smarthome") {
        Ok(token) => token,
        Err(error) => return error.into_json_response(None),
    };
    let user = token
        .user_id
        .and_then(|id| state.db.user_by_id(id).ok().flatten());
    let Some(user) = user else {
        return internal_error_response();
    };

    let specs: Vec<Value> = state
        .devices
        .devices
        .iter()
        .map(|(_, device)| devices::lock(device).core.specification())
        .collect();
    json_response(&json!({
        "request_id": request_id_value(&ctx.headers),
        "payload": {
            "user_id": user.username,
            "devices": specs,
        },
    }))
}

fn device_not_found(id: &Value) -> Value {
    json!({
        "id": id,
        "error_code": "DEVICE_NOT_FOUND",
        "error_message": "Устройство неизвестно",
    })
}

async fn query_devices(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    if let Err(error) = state.oauth.acquire_token(&ctx.headers, "smarthome") {
        return error.into_json_response(None);
    }
    let Ok(query) = serde_json::from_slice::<Value>(&ctx.body) else {
        return internal_error_response();
    };
    let Some(items) = query.get("devices").and_then(Value::as_array) else {
        return internal_error_response();
    };

    let mut result_devices: Vec<Value> = Vec::new();
    for item in items {
        let id = item.get("id").cloned().unwrap_or(Value::Null);
        let device = id.as_str().and_then(|id| state.devices.get(id));
        match device {
            Some(device) => result_devices.push(devices::lock(device).core.state()),
            None => result_devices.push(device_not_found(&id)),
        }
    }

    json_response(&json!({
        "request_id": request_id_value(&ctx.headers),
        "payload": {"devices": result_devices},
    }))
}

async fn control_devices(ctx: &Ctx) -> Response {
    let state = &ctx.state;
    if let Err(error) = state.oauth.acquire_token(&ctx.headers, "smarthome") {
        return error.into_json_response(None);
    }
    let Ok(query) = serde_json::from_slice::<Value>(&ctx.body) else {
        return internal_error_response();
    };
    let Some(items) = query
        .get("payload")
        .and_then(|p| p.get("devices"))
        .and_then(Value::as_array)
    else {
        return internal_error_response();
    };

    let mut result_devices: Vec<Value> = Vec::new();
    for item in items {
        let id = item.get("id").cloned().unwrap_or(Value::Null);
        let device = id.as_str().and_then(|id| state.devices.get(id));
        match device {
            Some(device) => {
                let empty = Vec::new();
                let capabilities = item
                    .get("capabilities")
                    .and_then(Value::as_array)
                    .unwrap_or(&empty);
                result_devices.push(devices::action(device, capabilities));
            }
            None => result_devices.push(device_not_found(&id)),
        }
    }

    json_response(&json!({
        "request_id": request_id_value(&ctx.headers),
        "payload": {"devices": result_devices},
    }))
}

// ------------------------------------------------------------------ dispatch

fn not_found() -> Response {
    plain(StatusCode::NOT_FOUND, "404: Not Found")
}

/// 405 with a text body plus the Allow header listing the permitted
/// methods.
fn method_not_allowed(allowed: &str) -> Response {
    let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "405: Method Not Allowed");
    if let Ok(value) = header::HeaderValue::from_str(allowed) {
        response.headers_mut().insert(header::ALLOW, value);
    }
    response
}

async fn route(ctx: &Ctx) -> Response {
    let path = ctx.uri.path();
    let rel = if ctx.state.prefix.is_empty() {
        path
    } else {
        match path.strip_prefix(&ctx.state.prefix) {
            Some(rest) if rest.starts_with('/') => rest,
            _ => return not_found(),
        }
    };

    // every GET route also serves HEAD
    let get = ctx.method == Method::GET || ctx.method == Method::HEAD;
    let post = ctx.method == Method::POST;

    match rel {
        "/auth" => {
            if get {
                auth_get(ctx).await
            } else if post {
                auth_post(ctx).await
            } else {
                method_not_allowed("GET,HEAD,POST")
            }
        }
        "/auth/logout" => {
            if post {
                logout_post(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        "/auth/register" if ctx.state.debug => {
            if post {
                register_post(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        "/oauth/create-client" if ctx.state.debug => {
            if get {
                create_client_get(ctx).await
            } else if post {
                create_client_post(ctx).await
            } else {
                method_not_allowed("GET,HEAD,POST")
            }
        }
        "/oauth/authorize" => {
            if get {
                oauthorize_get(ctx).await
            } else if post {
                oauthorize_post(ctx).await
            } else {
                method_not_allowed("GET,HEAD,POST")
            }
        }
        "/oauth/token" => {
            if get || post {
                token_any(ctx).await
            } else {
                method_not_allowed("GET,HEAD,POST")
            }
        }
        "/oauth/revoke" => {
            if post {
                revoke_post(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        "/me" => {
            if get {
                me_get(ctx).await
            } else {
                method_not_allowed("GET,HEAD")
            }
        }
        // Yandex probes the endpoint URL with HEAD {url}/v1.0, no slash
        "/v1.0" | "/v1.0/" => {
            if ctx.method == Method::HEAD {
                ping()
            } else {
                method_not_allowed("HEAD")
            }
        }
        "/v1.0/user/unlink" => {
            if post {
                unlink_post(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        "/v1.0/user/devices" => {
            if get {
                list_devices(ctx).await
            } else {
                method_not_allowed("GET,HEAD")
            }
        }
        "/v1.0/user/devices/query" => {
            if post {
                query_devices(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        "/v1.0/user/devices/action" => {
            if post {
                control_devices(ctx).await
            } else {
                method_not_allowed("POST")
            }
        }
        _ => not_found(),
    }
}

/// Handle one request end to end (routing + access log). Public so tests can
/// drive the real dispatcher without a socket.
pub async fn handle<B>(
    state: AppState,
    remote: Option<IpAddr>,
    request: http::Request<B>,
) -> Response
where
    B: hyper::body::Body,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (parts, body) = request.into_parts();
    let prefix = state.prefix.clone();
    let response = match Limited::new(body, MAX_BODY).collect().await {
        Ok(collected) => {
            let ctx = Ctx {
                state,
                method: parts.method.clone(),
                uri: parts.uri.clone(),
                headers: parts.headers.clone(),
                body: collected.to_bytes(),
            };
            if log_enabled!(target: "request", Level::Debug) {
                debug_request(&ctx);
            }
            let response = route(&ctx).await;
            if log_enabled!(target: "request", Level::Debug) {
                debug_response(&ctx, &response);
            }
            response
        }
        // an exceeded MAX_BODY gets a 413; the text cannot report the
        // actual body size because the limit aborts the read. Any other
        // body error (disconnect mid-body, malformed chunking) is a 400.
        Err(e)
            if e.downcast_ref::<http_body_util::LengthLimitError>()
                .is_some() =>
        {
            plain(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!("Maximum request body size {MAX_BODY} exceeded"),
            )
        }
        Err(_) => plain(StatusCode::BAD_REQUEST, "400: Bad Request"),
    };

    let addr = remote
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "-".to_string());
    let path_qs = loggable_target(&parts.uri, &prefix);
    fn header_or_dash(headers: &HeaderMap, name: &str) -> String {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string()
    }
    let referer = header_or_dash(&parts.headers, "referer");
    let user_agent = header_or_dash(&parts.headers, "user-agent");
    let request_id = header_or_dash(&parts.headers, "x-request-id");
    let length = hyper::body::Body::size_hint(response.body())
        .exact()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "-".to_string());
    info!(
        target: "access",
        "{addr} \"{} {path_qs} HTTP/1.1\" {} {length} \"{referer}\" \"{user_agent}\" \"{request_id}\"",
        parts.method,
        response.status().as_u16(),
    );
    response
}

/// Whether a request's query string and body may be shown in debug logs.
/// Decided on the same prefix-stripped path the router uses: the Yandex
/// API routes carry JSON without credentials; everything else (the auth
/// and oauth forms with passwords, secrets and tokens — which a sloppy
/// client might also put in the query) is never dumped.
fn api_route(ctx: &Ctx) -> bool {
    is_api_route(&ctx.uri, &ctx.state.prefix)
}

fn is_api_route(uri: &Uri, prefix: &str) -> bool {
    uri.path()
        .strip_prefix(prefix)
        .is_some_and(|rel| rel.starts_with("/v1.0/"))
}

/// Header values that may carry a credential are never shown.
fn sensitive_header(name: &header::HeaderName) -> bool {
    let name = name.as_str();
    ["auth", "cookie", "token", "secret", "key"]
        .iter()
        .any(|needle| name.contains(needle))
}

/// The request target as logged (access log and debug dump alike): the
/// path plus the query, the latter only on API routes.
fn loggable_target(uri: &Uri, prefix: &str) -> String {
    match uri.query() {
        Some(_) if !is_api_route(uri, prefix) => format!("{}?<redacted>", uri.path()),
        Some(query) => format!("{}?{query}", uri.path()),
        None => uri.path().to_string(),
    }
}

const DEBUG_BODY_LIMIT: usize = 4096;

fn debug_body(bytes: &[u8]) -> String {
    let shown = &bytes[..bytes.len().min(DEBUG_BODY_LIMIT)];
    let mut text = String::from_utf8_lossy(shown).into_owned();
    if bytes.len() > DEBUG_BODY_LIMIT {
        text.push_str(&format!("... ({} bytes total)", bytes.len()));
    }
    text
}

/// Debug dump of an incoming request: every header (credential-bearing
/// values redacted) and, for the API routes, the query and the body.
fn debug_request(ctx: &Ctx) {
    let headers: Vec<String> = ctx
        .headers
        .iter()
        .map(|(name, value)| {
            let shown = if sensitive_header(name) {
                "<redacted>".to_string()
            } else {
                format!("{:?}", String::from_utf8_lossy(value.as_bytes()))
            };
            format!("{name}: {shown}")
        })
        .collect();
    let body = if api_route(ctx) && !ctx.body.is_empty() {
        format!(" body={:?}", debug_body(&ctx.body))
    } else {
        format!(" body_len={}", ctx.body.len())
    };
    debug!(
        target: "request",
        "{} {} headers=[{}]{body}",
        ctx.method,
        loggable_target(&ctx.uri, &ctx.state.prefix),
        headers.join(", ")
    );
}

/// Debug dump of the response to an API route: status and body.
fn debug_response(ctx: &Ctx, response: &Response) {
    if !api_route(ctx) {
        return;
    }
    // the response body is a complete buffer, cloning it is a refcount bump
    let bytes = response.body().clone().into_inner().unwrap_or_default();
    debug!(
        target: "request",
        "-> {} {} {} body={:?}",
        ctx.method,
        ctx.uri.path(),
        response.status().as_u16(),
        debug_body(&bytes)
    );
}

/// Concurrent connections the server handles; beyond that, accepting
/// pauses and further connections wait in the kernel's listen backlog.
const MAX_CONNECTIONS: usize = 64;
/// Time a connection may take to deliver a request's headers, which also
/// bounds how long an idle keep-alive connection is kept (hyper's own
/// default). A fronting proxy that keeps upstream connections alive must
/// use a shorter idle timeout than this, or it may reuse a connection
/// that is being closed.
const HEADER_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Read buffer cap per connection; hyper would otherwise grow it to
/// ~400 KiB for dribbled headers.
const MAX_HEADER_BUFFER: usize = 16 * 1024;

/// Accept loop: one spawned task per connection, HTTP/1.1 only.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> anyhow::Result<()> {
    let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let mut saturated = false;
    loop {
        // a stalled or idle client must not hold a task, a socket and a
        // buffer forever: bounded concurrency, taken before accepting so a
        // flood queues in the kernel backlog, plus per-connection timeouts.
        // Saturation is logged once per episode, not per connection.
        let permit = match connections.clone().try_acquire_owned() {
            Ok(permit) => {
                saturated = false;
                permit
            }
            Err(_) => {
                if !saturated {
                    saturated = true;
                    log::warn!(
                        target: "server",
                        "{MAX_CONNECTIONS} connections open, new ones wait in the backlog"
                    );
                }
                let Ok(permit) = connections.clone().acquire_owned().await else {
                    break Ok(());
                };
                permit
            }
        };
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            // per-connection failures are retried immediately; resource
            // exhaustion (EMFILE & co) is persistent — back off so the
            // single-threaded runtime can run the tasks that release fds
            // instead of spinning on accept (asyncio and axum both wait 1s)
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                continue;
            }
            Err(e) => {
                log::error!(target: "server", "accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service = hyper::service::service_fn(move |request| {
                let state = state.clone();
                async move { Ok::<_, Infallible>(handle(state, Some(peer.ip()), request).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                // the timer makes the timeouts below effective; without it
                // hyper silently disables even its own defaults
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT)
                .max_buf_size(MAX_HEADER_BUFFER)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}
