//! End-to-end tests of the web server: routing, sessions, the full
//! OAuth flows, and golden assertions pinning the exact wire format
//! (bodies, headers, status codes) the account link depends on.

use std::sync::Arc;

use bytes::Bytes;
use http::{header, HeaderMap, Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use serde_json::Value;

use sorokdva_smarthome::db::{Client, Db, NewToken, Retire};
use sorokdva_smarthome::devices::{build_all, BuildContext};
use sorokdva_smarthome::oauth::OauthService;
use sorokdva_smarthome::session::SessionStore;
use sorokdva_smarthome::tasks::TaskSpawner;
use sorokdva_smarthome::web::{handle, AppState};

struct TestApp {
    state: AppState,
    db: Db,
}

fn make_app(debug: bool) -> TestApp {
    let db = Db::open(":memory:").unwrap();
    db.insert_user("username", "password").unwrap();
    let key = vec![7u8; 32];
    db.set_setting("cookie_key", &key).unwrap();

    let devices = build_all(
        &toml::Table::new(),
        &BuildContext {
            mqtt: None,
            tasks: TaskSpawner::new(),
        },
    )
    .unwrap();

    let state = AppState {
        db: db.clone(),
        oauth: OauthService { db: db.clone() },
        sessions: SessionStore::new(&key).unwrap(),
        devices: Arc::new(devices),
        prefix: String::new(),
        debug,
        proxy: false,
    };
    TestApp { state, db }
}

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    form: Option<&str>,
    json: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let body = if let Some(form) = form {
        builder = builder.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        Full::new(Bytes::from(form.to_string()))
    } else if let Some(json) = json {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Full::new(Bytes::from(json.to_string()))
    } else {
        Full::new(Bytes::new())
    };
    let request = builder.body(body).unwrap();
    let response = handle(app.state.clone(), None, request).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn add_client(db: &Db, allow_refresh: bool) -> Client {
    let client = Client {
        user_id: Some(1),
        client_id: "x".repeat(24),
        client_secret: "x".repeat(48),
        redirect_uri: "/auth".to_string(),
        token_endpoint_auth_method: "client_secret_post".to_string(),
        grant_type: if allow_refresh {
            "authorization_code\r\nrefresh_token".to_string()
        } else {
            "authorization_code".to_string()
        },
        response_type: "code".to_string(),
        scope: "smarthome another one".to_string(),
        client_name: "test".to_string(),
        client_uri: "http://localhost/".to_string(),
        ..Client::default()
    };
    db.insert_client(&client).unwrap();
    client
}

fn session_cookie(headers: &HeaderMap) -> String {
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("expected Set-Cookie")
        .to_str()
        .unwrap();
    cookie.split(';').next().unwrap().to_string()
}

async fn login(app: &TestApp) -> String {
    let (status, headers, _) = call(
        app,
        Method::POST,
        "/auth",
        &[],
        Some("username=username&password=password"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    session_cookie(&headers)
}

#[tokio::test]
async fn test_no_register() {
    let app = make_app(false);
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/auth/register",
        &[],
        Some("username=user1&password=password"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn test_authorize() {
    let app = make_app(false);

    let (status, headers, _) = call(
        &app,
        Method::POST,
        "/auth",
        &[],
        Some("username=username&password=password"),
        None,
    )
    .await;
    // assert the redirect itself plus the session cookie
    assert_eq!(status, StatusCode::FOUND);
    let cookie = session_cookie(&headers);
    let (status, _, _) = call(
        &app,
        Method::GET,
        "/auth",
        &[("cookie", &cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for form in [
        "username=username&password=wrong",
        "username=wrong&password=password",
    ] {
        let (status, headers, body) =
            call(&app, Method::POST, "/auth", &[], Some(form), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body, "Who are you? Go away!");
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/octet-stream"
        );
    }
}

async fn authorize_and_issue_token(allow_refresh: bool) {
    let app = make_app(false);
    let client = add_client(&app.db, allow_refresh);
    let cookie = login(&app).await;

    // request oauth authorization code page
    let authorize_qs = format!(
        "/oauth/authorize?client_id={}&scope=smarthome&response_type=code&state=TEST",
        client.client_id
    );
    let (status, _, body) = call(
        &app,
        Method::GET,
        &authorize_qs,
        &[("cookie", &cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("test is requesting"), "{body}");
    assert!(body.contains("smarthome"), "{body}");

    // confirm: 302 to redirect_uri with the code
    let (status, headers, body) = call(
        &app,
        Method::POST,
        &authorize_qs,
        &[("cookie", &cookie)],
        Some("confirm=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND, "{body}");
    let location = headers.get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(location.starts_with("/auth?code="), "{location}");
    assert!(location.ends_with("&state=TEST"), "{location}");
    let code = location
        .split("code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    assert_eq!(code.len(), 48);

    // exchange code for token
    let (status, headers, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}&code={}&grant_type=authorization_code",
            client.client_id, client.client_secret, code
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );
    assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    assert_eq!(headers.get(header::PRAGMA).unwrap(), "no-cache");

    let data: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(data["token_type"], "Bearer");
    let access_token = data["access_token"].as_str().unwrap().to_string();
    assert_eq!(access_token.len(), 42);
    assert_eq!(data["expires_in"], 31536000);
    assert_eq!(data["scope"], "smarthome");
    assert_eq!(allow_refresh, data.get("refresh_token").is_some());

    let token = app
        .db
        .token_by_access_token(&access_token)
        .unwrap()
        .unwrap();
    assert_eq!(token.scope, "smarthome");
    if allow_refresh {
        let refresh = data["refresh_token"].as_str().unwrap();
        assert_eq!(refresh.len(), 48);
        assert!(app.db.token_by_refresh_token(refresh).unwrap().is_some());
    }

    // replaying the code fails: it was deleted
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}&code={}&grant_type=authorization_code",
            client.client_id, client.client_secret, code
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        "{\"error\":\"invalid_grant\",\"error_description\":\"Invalid \\\"code\\\" in request.\"}"
    );

    if allow_refresh {
        // refresh: a fresh year-long access token, no new refresh token;
        // the refresh token stays valid and can be used again
        let refresh = data["refresh_token"].as_str().unwrap();
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
                client.client_id, client.client_secret, refresh
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let refreshed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(refreshed["expires_in"], 31536000);
        assert_eq!(refreshed["scope"], "smarthome");
        assert!(refreshed.get("refresh_token").is_none());

        let old = app.db.token_by_refresh_token(refresh).unwrap().unwrap();
        assert!(!old.revoked);

        // the same refresh token works again
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
                client.client_id, client.client_secret, refresh
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let again: Value = serde_json::from_str(&body).unwrap();
        assert_ne!(again["access_token"], refreshed["access_token"]);
        // the previous refreshed access token is retired, the root stays
        assert!(app
            .db
            .token_by_access_token(refreshed["access_token"].as_str().unwrap())
            .unwrap()
            .is_none());
        assert!(app
            .db
            .token_by_access_token(data["access_token"].as_str().unwrap())
            .unwrap()
            .is_some());

        // revoking the access token the client holds after a refresh
        // unlinks the whole account: the refresh token dies with it
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/revoke",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&token={}",
                client.client_id,
                client.client_secret,
                again["access_token"].as_str().unwrap()
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(app.db.token_by_refresh_token(refresh).unwrap().is_none());
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
                client.client_id, client.client_secret, refresh
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"error\":\"invalid_grant\"}");

        // an unknown refresh token -> invalid_grant with no description
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&refresh_token=nope&grant_type=refresh_token",
                client.client_id, client.client_secret
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"error\":\"invalid_grant\"}");

        // re-linking (a new code grant) supersedes the previous link: the
        // new token pair works, the old refresh token does not
        let (_, headers, _) = call(
            &app,
            Method::POST,
            &authorize_qs,
            &[("cookie", &cookie)],
            Some("confirm=true"),
            None,
        )
        .await;
        let location = headers.get(header::LOCATION).unwrap().to_str().unwrap();
        let code = location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&code={}&grant_type=authorization_code",
                client.client_id, client.client_secret, code
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let relinked: Value = serde_json::from_str(&body).unwrap();
        let new_refresh = relinked["refresh_token"].as_str().unwrap();
        assert!(app
            .db
            .token_by_refresh_token(new_refresh)
            .unwrap()
            .is_some());

        // and once more: the second re-link retires the first one's pair
        let (_, headers, _) = call(
            &app,
            Method::POST,
            &authorize_qs,
            &[("cookie", &cookie)],
            Some("confirm=true"),
            None,
        )
        .await;
        let location = headers.get(header::LOCATION).unwrap().to_str().unwrap();
        let code = location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/token",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&code={}&grant_type=authorization_code",
                client.client_id, client.client_secret, code
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let relinked_again: Value = serde_json::from_str(&body).unwrap();
        let newest_refresh = relinked_again["refresh_token"].as_str().unwrap();
        assert!(app
            .db
            .token_by_refresh_token(newest_refresh)
            .unwrap()
            .is_some());
        assert!(app
            .db
            .token_by_refresh_token(new_refresh)
            .unwrap()
            .is_none());
        assert!(app
            .db
            .token_by_access_token(relinked["access_token"].as_str().unwrap())
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn test_authorize_and_issue_token_with_refresh() {
    authorize_and_issue_token(true).await;
}

#[tokio::test]
async fn test_authorize_and_issue_token_without_refresh() {
    authorize_and_issue_token(false).await;
}

#[tokio::test]
async fn test_authorize_error_responses() {
    let app = make_app(false);
    add_client(&app.db, true);
    let cookie = login(&app).await;

    // unknown client -> 400 {"error": "invalid_client", "state": "TEST"}
    let (status, headers, body) = call(
        &app,
        Method::GET,
        "/oauth/authorize?client_id=unknown&scope=s&response_type=code&state=TEST",
        &[("cookie", &cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "{\"error\":\"invalid_client\",\"state\":\"TEST\"}");
    assert_eq!(
        headers.get(header::CONTENT_TYPE).unwrap(),
        "application/json; charset=utf-8"
    );

    // bad redirect uri
    let (status, _, body) = call(
        &app,
        Method::GET,
        &format!(
            "/oauth/authorize?client_id={}&response_type=code&state=TEST&redirect_uri=http://evil/",
            "x".repeat(24)
        ),
        &[("cookie", &cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        "{\"error\":\"invalid_request\",\"error_description\":\
         \"Redirect URI http://evil/ is not supported by client.\",\"state\":\"TEST\"}"
    );

    // GET with response_type=token -> 400 unsupported_response_type
    let (status, _, body) = call(
        &app,
        Method::GET,
        &format!(
            "/oauth/authorize?client_id={}&response_type=token&state=TEST",
            "x".repeat(24)
        ),
        &[("cookie", &cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "{\"error\":\"unsupported_response_type\"}");

    // POST with response_type=token -> uncaught UnsupportedResponseTypeError -> 500
    let (status, _, _) = call(
        &app,
        Method::POST,
        &format!(
            "/oauth/authorize?client_id={}&response_type=token&state=TEST",
            "x".repeat(24)
        ),
        &[("cookie", &cookie)],
        Some("confirm=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // POST without consent -> access_denied redirect (exact encoding)
    let (status, headers, _) = call(
        &app,
        Method::POST,
        &format!(
            "/oauth/authorize?client_id={}&scope=smarthome&response_type=code&state=TEST",
            "x".repeat(24)
        ),
        &[("cookie", &cookie)],
        Some(""),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(
        headers.get(header::LOCATION).unwrap(),
        "/auth?error=access_denied&error_description=The+resource+owner+or+authorization+server+\
         denied+the+request&state=TEST"
    );

    // POST not logged in -> 401 text
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/authorize?client_id=x&response_type=code",
        &[],
        Some("confirm=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, "401: Unauthorized");

    // GET not logged in -> 302 to /auth
    let (status, headers, _) = call(
        &app,
        Method::GET,
        "/oauth/authorize?client_id=x&response_type=code",
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/auth");
}

#[tokio::test]
async fn test_token_endpoint_errors() {
    let app = make_app(false);
    let client = add_client(&app.db, true);

    // bad secret -> 400 invalid_client
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        Some(&format!(
            "client_id={}&client_secret=wrong&code=zzz&grant_type=authorization_code",
            client.client_id
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "{\"error\":\"invalid_client\"}");

    // missing code
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}&grant_type=authorization_code",
            client.client_id, client.client_secret
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        "{\"error\":\"invalid_request\",\"error_description\":\
         \"Missing \\\"code\\\" in request.\"}"
    );

    // unsupported grant type -> uncaught -> 500
    let (status, _, _) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        Some("grant_type=password&username=u&password=p"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // GET -> uncaught -> 500
    let (status, _, _) = call(&app, Method::GET, "/oauth/token", &[], None, None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_revocation() {
    let app = make_app(false);
    let client = add_client(&app.db, true);
    app.db
        .insert_token(
            NewToken {
                client_id: &client.client_id,
                user_id: Some(1),
                access_token: "xxx",
                refresh_token: Some("yyy"),
                scope: Some("smarthome"),
                issued_at: sorokdva_smarthome::oauth::now(),
                expires_in: 600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();

    // missing token parameter
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/revoke",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}",
            client.client_id, client.client_secret
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "{\"error\":\"invalid_request\"}");

    // bad token_type_hint -> 401 unsupported_token_type
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/revoke",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}&token=xxx&token_type_hint=bad",
            client.client_id, client.client_secret
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body, "{\"error\":\"unsupported_token_type\"}");

    // successful revocation (and unknown tokens also give 200 {})
    for token in ["yyy", "unknown"] {
        let (status, _, body) = call(
            &app,
            Method::POST,
            "/oauth/revoke",
            &[],
            Some(&format!(
                "client_id={}&client_secret={}&token={}",
                client.client_id, client.client_secret, token
            )),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "{}");
    }
    // revoked tokens are pruned from the store
    assert!(app.db.token_by_access_token("xxx").unwrap().is_none());
}

#[tokio::test]
async fn test_fetch_without_authorization() {
    let app = make_app(false);

    let (status, _, _) = call(
        &app,
        Method::HEAD,
        "/v1.0/",
        &[("authorization", "Bearer garbage")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Yandex's endpoint probe has no trailing slash
    let (status, _, _) = call(&app, Method::HEAD, "/v1.0", &[], None, None).await;
    assert_eq!(status, StatusCode::OK);

    // GET /v1.0/ -> 405 with Allow
    let (status, headers, _) = call(&app, Method::GET, "/v1.0/", &[], None, None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(headers.get(header::ALLOW).is_some());

    let (status, headers, body) = call(
        &app,
        Method::GET,
        "/v1.0/user/devices",
        &[("authorization", "Bearer garbage")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(
        headers.get(header::WWW_AUTHENTICATE).unwrap(),
        "Bearer extra_attributes=\"{}\", error=\"invalid_token\", error_description=\"The access \
         token provided is expired, revoked, malformed, or invalid for other reasons.\"
"
        .trim_end(),
    );

    // no Authorization header at all
    let (status, headers, body) =
        call(&app, Method::GET, "/v1.0/user/devices", &[], None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        body,
        "{\"error\":\"missing_authorization\",\"error_description\":\
         \"Missing \\\"Authorization\\\" in headers.\"}"
    );
    assert_eq!(
        headers.get(header::WWW_AUTHENTICATE).unwrap(),
        "bearer error=\"missing_authorization\", error_description=\"Missing \"Authorization\" \
         in headers.\""
    );

    // non-bearer scheme
    let (status, _, body) = call(
        &app,
        Method::GET,
        "/v1.0/user/devices",
        &[("authorization", "Basic dXNlcjpwYXNz")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        body.starts_with("{\"error\":\"unsupported_token_type\""),
        "{body}"
    );

    for uri in [
        "/v1.0/user/unlink",
        "/v1.0/user/devices/query",
        "/v1.0/user/devices/action",
    ] {
        let (status, _, body) = call(
            &app,
            Method::POST,
            uri,
            &[("authorization", "Bearer garbage")],
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}: {body}");
    }
}

#[tokio::test]
async fn test_fetch_with_authorization() {
    let app = make_app(false);
    let client = Client {
        user_id: Some(1),
        client_id: "client".to_string(),
        client_secret: "secret".to_string(),
        redirect_uri: "/auth".to_string(),
        token_endpoint_auth_method: "client_secret_post".to_string(),
        grant_type: "authorization_code".to_string(),
        response_type: "code".to_string(),
        scope: "smarthome".to_string(),
        client_name: "test".to_string(),
        client_uri: "http://localhost/".to_string(),
        ..Client::default()
    };
    app.db.insert_client(&client).unwrap();
    app.db
        .insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(1),
                access_token: "xxx",
                refresh_token: Some("yyy"),
                scope: Some("smarthome"),
                issued_at: sorokdva_smarthome::oauth::now(),
                expires_in: 600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();

    let auth = [("authorization", "Bearer xxx")];

    let (status, _, body) = call(&app, Method::GET, "/v1.0/user/devices", &auth, None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let data: Value = serde_json::from_str(&body).unwrap();
    assert!(data.get("payload").is_some());
    assert_eq!(data["payload"]["user_id"], "username");
    assert_eq!(data["request_id"], Value::Null);

    let (status, _, body) = call(
        &app,
        Method::POST,
        "/v1.0/user/devices/query",
        &auth,
        None,
        Some("{\"devices\":[]}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(serde_json::from_str::<Value>(&body)
        .unwrap()
        .get("payload")
        .is_some());

    let (status, _, body) = call(
        &app,
        Method::POST,
        "/v1.0/user/devices/query",
        &auth,
        None,
        Some("{\"devices\":[{\"id\":\"nope\"}]}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(
            "{\"error_code\":\"DEVICE_NOT_FOUND\",\"error_message\":\"Устройство неизвестно\",\"id\":\"nope\"}"
        ),
        "{body}"
    );

    let (status, _, body) = call(
        &app,
        Method::POST,
        "/v1.0/user/devices/action",
        &auth,
        None,
        Some("{\"payload\":{\"devices\":[]}}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(serde_json::from_str::<Value>(&body)
        .unwrap()
        .get("payload")
        .is_some());

    // unlink carries only the bearer token and retires its whole link
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/v1.0/user/unlink",
        &[("authorization", "Bearer xxx"), ("x-request-id", "req-1")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "{\"request_id\":\"req-1\"}");
    assert!(app.db.token_by_refresh_token("yyy").unwrap().is_none());

    // token is now revoked
    let (status, _, _) = call(&app, Method::GET, "/v1.0/user/devices", &auth, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_profile_scopes() {
    let app = make_app(false);
    let now = sorokdva_smarthome::oauth::now();
    app.db
        .insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(1),
                access_token: "xxx1",
                refresh_token: Some("yyy1"),
                scope: Some("smarthome"),
                issued_at: now,
                expires_in: 600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();
    app.db
        .insert_token(
            NewToken {
                client_id: "client",
                user_id: Some(1),
                access_token: "xxx2",
                refresh_token: Some("yyy2"),
                scope: Some("smarthome profile"),
                issued_at: now,
                expires_in: 600,
            },
            Retire::Nothing,
            None,
        )
        .unwrap();

    let (status, _, body) = call(
        &app,
        Method::GET,
        "/me",
        &[("authorization", "Bearer xxx1")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        body,
        "{\"error\":\"insufficient_scope\",\"error_description\":\"The request requires \
         higher privileges than provided by the access token.\"}"
    );

    let (status, _, body) = call(
        &app,
        Method::GET,
        "/me",
        &[("authorization", "Bearer xxx2")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "{\"id\":1,\"username\":\"username\"}");
}

#[tokio::test]
async fn test_debug_register_and_client() {
    let app = make_app(true);

    let (status, headers, _) = call(
        &app,
        Method::POST,
        "/auth/register",
        &[],
        Some("username=user1&password=password"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/auth");
    let cookie = session_cookie(&headers);

    // duplicate -> 400
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/auth/register",
        &[],
        Some("username=user1&password=password"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "User already exists");

    let (status, _, _) = call(
        &app,
        Method::POST,
        "/oauth/create-client",
        &[("cookie", &cookie)],
        Some(
            "client_name=test&client_uri=http://localhost/&scope=smarthome&\
             redirect_uri=https://social.yandex.net/broker/redirect&\
             grant_type=authorization_code%0D%0Arefresh_token&response_type=code",
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);

    let clients = app.db.clients_all().unwrap();
    assert_eq!(clients.len(), 1);
    let client = &clients[0];
    assert_eq!(client.client_id.len(), 24);
    assert_eq!(client.client_secret.len(), 48);
    assert_eq!(client.token_endpoint_auth_method, "client_secret_post");
    assert_eq!(
        client.grant_types(),
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string()
        ]
    );
}

#[tokio::test]
async fn test_prefix_and_logout() {
    let db = Db::open(":memory:").unwrap();
    db.insert_user("username", "password").unwrap();
    let key = vec![7u8; 32];
    db.set_setting("cookie_key", &key).unwrap();
    let devices = build_all(
        &toml::Table::new(),
        &BuildContext {
            mqtt: None,
            tasks: TaskSpawner::new(),
        },
    )
    .unwrap();
    let state = AppState {
        db: db.clone(),
        oauth: OauthService { db: db.clone() },
        sessions: SessionStore::new(&key).unwrap(),
        devices: Arc::new(devices),
        prefix: "/alice".to_string(),
        debug: false,
        proxy: true,
    };
    let app = TestApp { state, db };

    // outside prefix -> 404 with the plain-text body
    let (status, _, body) = call(&app, Method::GET, "/auth", &[], None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, "404: Not Found");

    // login under prefix: absolute Location with X-Forwarded honoured
    let (status, headers, _) = call(
        &app,
        Method::POST,
        "/alice/auth?next=1",
        &[
            ("host", "internal:8080"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "mydomain.com"),
        ],
        Some("username=username&password=password"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(
        headers.get(header::LOCATION).unwrap(),
        "https://mydomain.com/alice/auth?next=1"
    );
    let set_cookie = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(set_cookie.starts_with("session=\""), "{set_cookie}");
    assert!(set_cookie.ends_with("; HttpOnly; Path=/"), "{set_cookie}");
    let login_cookie = session_cookie(&headers);

    // authorize redirect when unauthenticated points inside the prefix
    let (status, headers, _) = call(
        &app,
        Method::GET,
        "/alice/oauth/authorize?client_id=x&response_type=code",
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/alice/auth");

    // logout with a live session clears the session cookie
    let (status, headers, _) = call(
        &app,
        Method::POST,
        "/alice/auth/logout?a=b",
        &[("cookie", &login_cookie)],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/alice/auth?a=b");
    let set_cookie = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert_eq!(
        set_cookie,
        "session=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; HttpOnly; Path=/"
    );

    // logout without a session sends no cookie at all (an untouched
    // session is not re-saved)
    let (status, headers, _) =
        call(&app, Method::POST, "/alice/auth/logout", &[], None, None).await;
    assert_eq!(status, StatusCode::FOUND);
    assert!(headers.get(header::SET_COOKIE).is_none());
}

/// Client credentials, grant_type, code/refresh_token, token and
/// confirm are read from the POST body only — query-string values must
/// not work.
#[tokio::test]
async fn test_form_only_parameters() {
    let app = make_app(false);
    let client = add_client(&app.db, true);
    let cookie = login(&app).await;

    // credentials + code in the query string only -> invalid_client
    let (status, _, body) = call(
        &app,
        Method::POST,
        &format!(
            "/oauth/token?client_id={}&client_secret={}&code=z",
            client.client_id, client.client_secret
        ),
        &[],
        Some("grant_type=authorization_code"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body, "{\"error\":\"invalid_client\"}");

    // grant_type in the query only -> uncaught UnsupportedGrantTypeError -> 500
    let (status, _, _) = call(
        &app,
        Method::POST,
        "/oauth/token?grant_type=authorization_code",
        &[],
        Some(""),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // revoke with the token in the query only -> invalid_request
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/revoke?token=xxx",
        &[],
        Some(&format!(
            "client_id={}&client_secret={}",
            client.client_id, client.client_secret
        )),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "{\"error\":\"invalid_request\"}");

    // confirm in the query only counts as no consent -> access_denied redirect
    let (status, headers, _) = call(
        &app,
        Method::POST,
        &format!(
            "/oauth/authorize?client_id={}&response_type=code&confirm=true",
            client.client_id
        ),
        &[("cookie", &cookie)],
        Some(""),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    let location = headers.get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(location.contains("error=access_denied"), "{location}");

    // a non-form content type means no form at all
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[],
        None,
        Some(&format!(
            "{{\"client_id\":\"{}\",\"client_secret\":\"{}\",\
              \"grant_type\":\"authorization_code\",\"code\":\"z\"}}",
            client.client_id, client.client_secret
        )),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
}

/// The Content-Type mimetype is parsed case-insensitively with its
/// parameters ignored; a missing header means no form at all.
#[tokio::test]
async fn test_content_type_normalization() {
    let app = make_app(false);
    let client = add_client(&app.db, true);

    // uppercase + parameters still counts as a form
    let (status, _, body) = call(
        &app,
        Method::POST,
        "/oauth/token",
        &[(
            "content-type",
            "Application/X-WWW-Form-Urlencoded; charset=UTF-8",
        )],
        None,
        Some(&format!(
            "client_id={}&client_secret=wrong&grant_type=authorization_code&code=z",
            client.client_id
        )),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body, "{\"error\":\"invalid_client\"}");

    // no Content-Type header counts as application/octet-stream:
    // no form -> unsupported grant -> 500
    let request = Request::builder()
        .method(Method::POST)
        .uri("/oauth/token")
        .body(Full::new(Bytes::from("grant_type=authorization_code")))
        .unwrap();
    let response = handle(app.state.clone(), None, request).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// An empty Content-Type value counts as text/plain: no form.
#[tokio::test]
async fn test_empty_content_type_is_not_a_form() {
    let app = make_app(false);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/oauth/token")
        .header(header::CONTENT_TYPE, "")
        .body(Full::new(Bytes::from("grant_type=authorization_code")))
        .unwrap();
    let response = handle(app.state.clone(), None, request).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// An oversized request body (over 1 MiB) is rejected with 413.
#[tokio::test]
async fn test_body_size_limit() {
    let app = make_app(false);
    let big = "x".repeat(1024 * 1024 + 1);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/oauth/token")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Full::new(Bytes::from(big)))
        .unwrap();
    let response = handle(app.state.clone(), None, request).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"Maximum request body size 1048576 exceeded");

    // exactly at the limit is accepted (reaches the 500 unsupported-grant path)
    let ok = "x".repeat(1024 * 1024);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/oauth/token")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Full::new(Bytes::from(ok)))
        .unwrap();
    let response = handle(app.state.clone(), None, request).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
