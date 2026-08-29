//! The three HTML pages: index (login/registration and the client
//! list), the oauth consent form, and the create-client form.

use serde_json::{json, Map};

use crate::db::{AuthCode, Client, Token, User};

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn opt_str(value: &Option<String>) -> String {
    match value {
        Some(v) => v.clone(),
        None => "None".to_string(),
    }
}

fn opt_i64(value: Option<i64>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => "None".to_string(),
    }
}

fn client_info_json(client: &Client) -> String {
    serde_json::to_string(&json!({
        "client_id": client.client_id,
        "client_secret": client.client_secret,
        "client_id_issued_at": client.issued_at,
        "client_secret_expires_at": client.expires_at,
    }))
    .unwrap_or_default()
}

fn client_metadata_json(client: &Client) -> String {
    let mut metadata = Map::new();
    metadata.insert("redirect_uris".into(), json!(client.redirect_uris()));
    metadata.insert(
        "token_endpoint_auth_method".into(),
        json!(client.token_endpoint_auth_method),
    );
    metadata.insert("grant_types".into(), json!(client.grant_types()));
    metadata.insert("response_types".into(), json!(client.response_types()));
    metadata.insert("client_name".into(), json!(client.client_name));
    metadata.insert("client_uri".into(), json!(client.client_uri));
    metadata.insert("scope".into(), json!(client.scope));
    serde_json::to_string(&metadata).unwrap_or_default()
}

fn code_str(code: &AuthCode) -> String {
    format!(
        "AuthorizationCode id={} user_id={} client_id={} code={} redirect_uri={} \
         scope={} auth_time={}",
        code.id,
        opt_i64(code.user_id),
        code.client_id,
        code.code,
        code.redirect_uri,
        code.scope,
        code.auth_time,
    )
}

fn token_str(token: &Token) -> String {
    format!(
        "Token id={} user_id={} client_id={} access_token={} \
         refresh_token={} scope={} revoked={} issued_at={} expires_in={}",
        token.id,
        opt_i64(token.user_id),
        token.client_id,
        token.access_token,
        opt_str(&token.refresh_token),
        token.scope,
        if token.revoked { "True" } else { "False" },
        token.issued_at,
        token.expires_in,
    )
}

pub struct IndexContext<'a> {
    pub user: Option<&'a User>,
    pub clients: &'a [Client],
    pub codes: &'a [(AuthCode, Option<Client>)],
    pub tokens: &'a [(Token, Option<Client>)],
    pub post_query: &'a str,
    pub debug: bool,
    /// url_for closure input: route name -> path
    pub prefix: &'a str,
}

fn url_for(prefix: &str, path: &str) -> String {
    format!("{prefix}{path}")
}

pub fn index_page(ctx: &IndexContext) -> String {
    let mut body = String::from(
        "<!DOCTYPE html>\n<html>\n    <head>\n        <title>Next door to Alice</title>\n        \
         <style content-type=\"text/css\">\n            pre {\n                white-space: wrap;\n            \
         }\n        </style>\n    </head>\n    <body>\n",
    );

    if let Some(user) = ctx.user {
        body.push_str(&format!(
            "<div>Logged in as <strong>{}</strong> \
             <form method=\"POST\" action=\"{}?{}\"><button>Log Out</button></form></div>\n",
            html_escape(&user.username),
            url_for(ctx.prefix, "/auth/logout"),
            ctx.post_query,
        ));

        if !ctx.clients.is_empty() {
            body.push_str("Clients:\n");
            for client in ctx.clients {
                body.push_str(&format!(
                    "<pre>\n{}\n{}\n</pre>\n",
                    html_escape(&client_info_json(client)),
                    html_escape(&client_metadata_json(client)),
                ));
                if ctx.debug {
                    body.push_str(&format!(
                        "<form method=\"GET\" action=\"{}\">\n\
                         <input type=\"hidden\" name=\"client_id\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"scope\" value=\"profile offline_access smarthome\" />\n\
                         <input type=\"hidden\" name=\"response_type\" value=\"code\" />\n\
                         <input type=\"hidden\" name=\"state\" value=\"TEST\" />\n\
                         <button>Create code</button>\n</form>\n",
                        url_for(ctx.prefix, "/oauth/authorize"),
                        html_escape(&client.client_id),
                    ));
                } else {
                    body.push_str(&format!(
                        "<a href=\"{}?{}\">Create code</a>\n",
                        url_for(ctx.prefix, "/oauth/authorize"),
                        ctx.post_query,
                    ));
                }
                body.push_str("<hr/>\n");
            }
        }

        if !ctx.codes.is_empty() {
            body.push_str("Codes:\n");
            for (code, client) in ctx.codes {
                body.push_str(&format!(
                    "<pre>\n{}\n</pre>\n",
                    html_escape(&code_str(code))
                ));
                if ctx.debug {
                    let secret = client
                        .as_ref()
                        .map(|c| c.client_secret.clone())
                        .unwrap_or_default();
                    body.push_str(&format!(
                        "<form method=\"POST\" action=\"{}\">\n\
                         <input type=\"hidden\" name=\"client_id\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"client_secret\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"code\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"grant_type\" value=\"authorization_code\" />\n\
                         <button>Create token</button>\n</form>\n",
                        url_for(ctx.prefix, "/oauth/token"),
                        html_escape(&code.client_id),
                        html_escape(&secret),
                        html_escape(&code.code),
                    ));
                }
                body.push_str("<hr/>\n");
            }
        }

        if !ctx.tokens.is_empty() {
            body.push_str("Tokens:\n");
            for (token, client) in ctx.tokens {
                body.push_str(&format!(
                    "<pre>\n{}\n</pre>\n",
                    html_escape(&token_str(token))
                ));
                if ctx.debug && !token.revoked {
                    let secret = client
                        .as_ref()
                        .map(|c| c.client_secret.clone())
                        .unwrap_or_default();
                    let value = token
                        .refresh_token
                        .clone()
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| token.access_token.clone());
                    body.push_str(&format!(
                        "<form method=\"POST\" action=\"{}\">\n\
                         <input type=\"hidden\" name=\"client_id\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"client_secret\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"token\" value=\"{}\" />\n\
                         <input type=\"hidden\" name=\"grant_type\" value=\"authorization_code\" />\n\
                         <button>Revoke token</button>\n</form>\n",
                        url_for(ctx.prefix, "/oauth/revoke"),
                        html_escape(&token.client_id),
                        html_escape(&secret),
                        html_escape(&value),
                    ));
                }
                body.push_str("<hr/>\n");
            }
        }

        if ctx.debug {
            body.push_str(&format!(
                "<br/>\n<a href=\"{}\">Create Client</a>\n",
                url_for(ctx.prefix, "/oauth/create-client"),
            ));
        }
    } else {
        body.push_str(&format!(
            "<form method=\"POST\" action=\"?{}\">\n\
             <input type=\"text\" name=\"username\" placeholder=\"Login\" /><br/>\n\
             <input type=\"password\" name=\"password\" placeholder=\"Password\" /><br/>\n\
             <button>Sign in</button>\n",
            ctx.post_query,
        ));
        if ctx.debug {
            body.push_str(&format!(
                "<button formaction=\"{}\">Register</button>\n",
                url_for(ctx.prefix, "/auth/register"),
            ));
        }
        body.push_str("</form>\n");
    }

    body.push_str("    </body>\n</html>\n");
    body
}

pub fn oauth_page(client_name: &str, scope: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html>\n    <head>\n        <title>Next door to Alice</title>\n    </head>\n    \
         <body>\n        <p>\n            {} is requesting:\n            <strong>{}</strong>\n        </p>\n\n        \
         <form action=\"\" method=\"POST\">\n            <label for=\"confirm\">\n                \
         <input type=\"checkbox\" name=\"confirm\">\n                <span>Consent?</span>\n            \
         </label>\n            <button>Submit</button>\n        </form>\n    </body>\n</html>\n",
        html_escape(client_name),
        html_escape(scope),
    )
}

pub fn create_client_page(prefix: &str) -> String {
    let auth_url = format!("{prefix}/auth");
    format!(
        "<!DOCTYPE html>\n<html>\n    <head>\n        <title>Next door to Alice</title>\n        \
         <style content-type=\"text/css\">\n            label, label > span {{\n                display: block;\n            }}\n            \
         label {{\n                margin: 15px 0;\n            }}\n        </style>\n    </head>\n    <body>\n        \
         <a href=\"{}\">Home</a>\n\n        <form action=\"\" method=\"POST\">\n            \
         <label for=\"client_name\">\n                <span>Client Name</span>\n                \
         <input type=\"text\" name=\"client_name\" />\n            </label>\n            \
         <label for=\"client_uri\">\n                <span>Client URI</span>\n                \
         <input type=\"url\" name=\"client_uri\" />\n            </label>\n            \
         <label for=\"scope\">\n                <span>Allowed Scope</span>\n                \
         <input type=\"text\" name=\"scope\" />\n            </label>\n            \
         <label for='redirect_uri'>\n                <span>Redirect URIs</span>\n                \
         <textarea name=\"redirect_uri\" cols=\"30\" rows=\"10\"></textarea>\n            </label>\n            \
         <label for=\"grant_type\">\n                <span>Allowed Grant Types</span>\n                \
         <textarea name=\"grant_type\" cols=\"30\" rows=\"10\"></textarea>\n            </label>\n            \
         <label for=\"response_type\">\n                <span>Allowed Response Types</span>\n                \
         <textarea name=\"response_type\" cols=\"30\" rows=\"10\"></textarea>\n            </label>\n            \
         <button>Submit</button>\n        </form>\n    </body>\n</html>\n",
        auth_url,
    )
}
