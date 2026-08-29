use std::sync::Arc;

use anyhow::{Context, Result};

use sorokdva_smarthome::config;
use sorokdva_smarthome::db::Db;
use sorokdva_smarthome::devices::{self, BuildContext};
use sorokdva_smarthome::mqtt;
use sorokdva_smarthome::notifications::Notifications;
use sorokdva_smarthome::oauth::OauthService;
use sorokdva_smarthome::session::SessionStore;
use sorokdva_smarthome::tasks::TaskSpawner;
use sorokdva_smarthome::web::{serve, AppState};

struct Args {
    interface: String,
    port: u16,
    prefix: String,
    db: String,
    debug: bool,
    proxy: bool,
    cfg: String,
}

fn parse_args() -> Result<Args> {
    use lexopt::prelude::*;

    let mut args = Args {
        interface: "127.0.0.1".to_string(),
        port: 8080,
        prefix: "/".to_string(),
        db: ":memory:".to_string(),
        debug: std::env::var_os("DIALOGS_DEBUG").is_some_and(|v| !v.is_empty()),
        proxy: false,
        cfg: "app.toml".to_string(),
    };

    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('i') | Long("interface") => args.interface = parser.value()?.string()?,
            Short('p') | Long("port") => args.port = parser.value()?.parse()?,
            Long("prefix") => args.prefix = parser.value()?.string()?,
            Long("db") => args.db = parser.value()?.string()?,
            Long("debug") => args.debug = true,
            Long("proxy") => args.proxy = true,
            Long("cfg") => args.cfg = parser.value()?.string()?,
            Short('h') | Long("help") => {
                println!(
                    "usage: sorokdva-smarthome [-i INTERFACE] [-p PORT] [--prefix PREFIX] \
                     [--db FILENAME] [--debug] [--proxy] [--cfg CFG]"
                );
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected().into()),
        }
    }
    Ok(args)
}

fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn main() -> Result<()> {
    let args = parse_args()?;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(if args.debug {
        "debug"
    } else {
        "info"
    }))
    .init();

    if args.debug && !matches!(args.interface.as_str(), "127.0.0.1" | "localhost" | "::1") {
        log::warn!(
            target: "server",
            "--debug on {}: the unauthenticated /auth/register route and the request dump \
             are exposed beyond loopback",
            args.interface
        );
    }

    let cfg = config::load(&args.cfg)?;

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot build runtime")?
        .block_on(run(args, cfg))
}

async fn run(args: Args, cfg: config::Config) -> Result<()> {
    let db = Db::open(&args.db)?;

    let cookie_key = match db.setting("cookie_key")? {
        Some(key) => key,
        None => {
            let mut key = vec![0u8; 32];
            getrandom::getrandom(&mut key).context("cannot generate cookie key")?;
            db.set_setting("cookie_key", &key)?;
            key
        }
    };
    let sessions = SessionStore::new(&cookie_key).context("invalid cookie key in database")?;

    let tasks = TaskSpawner::new();
    let (mqtt_handle, mqtt_rx) = if cfg.mqtt.is_some() {
        let (handle, rx) = mqtt::channel();
        (Some(handle), Some(rx))
    } else {
        (None, None)
    };

    let built = devices::build_all(
        &cfg.devices,
        &BuildContext {
            mqtt: mqtt_handle,
            tasks: tasks.clone(),
        },
    )?;
    let built = Arc::new(built);

    // event transitions bypass the periodic sampler; the channel only
    // exists when there is a notifications loop to consume it
    let (state_sink, state_events) = if cfg.notifications.is_some() {
        let (sink, events) = devices::StateSink::channel();
        (Some(sink), Some(events))
    } else {
        (None, None)
    };

    if let (Some(mqtt_cfg), Some(rx)) = (cfg.mqtt.clone(), mqtt_rx) {
        tokio::spawn(mqtt::run(
            mqtt_cfg,
            rx,
            built.clone(),
            tasks.clone(),
            state_sink,
        ));
    }

    // discovery runs before serving; a failure aborts startup
    let notifications = match &cfg.notifications {
        Some(ncfg) => {
            let notifications = Notifications::new(ncfg.clone())?;
            notifications.send_discovery().await?;
            Some(Arc::new(notifications))
        }
        None => None,
    };

    // per-device background poll loops; MQTT devices have none
    devices::spawn_pollers(&built);

    if let (Some(notifications), Some(events)) = (notifications, state_events) {
        let built = built.clone();
        tokio::spawn(async move { notifications.notifications_loop(built, events).await });
    }

    let state = AppState {
        db: db.clone(),
        oauth: OauthService { db },
        sessions,
        devices: built,
        prefix: normalize_prefix(&args.prefix),
        debug: args.debug,
        proxy: args.proxy,
    };

    let listener = tokio::net::TcpListener::bind((args.interface.as_str(), args.port))
        .await
        .with_context(|| format!("cannot bind {}:{}", args.interface, args.port))?;
    log::info!(
        target: "server",
        "Running on http://{}:{}", args.interface, args.port,
    );
    serve(listener, state).await
}
