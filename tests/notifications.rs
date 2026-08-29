//! The event-transition notification path: a PIR pulse shorter than the
//! sampling period must still deliver both edges to Yandex.

#![cfg(feature = "wirenboard")]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use sorokdva_smarthome::config::NotificationsConfig;
use sorokdva_smarthome::devices::{self, BuildContext, BuiltDevices, StateChange, StateSink};
use sorokdva_smarthome::mqtt;
use sorokdva_smarthome::notifications::{
    drain_batch, enqueue, full_pass, merge_reported, Notifications, MAX_PENDING,
};
use sorokdva_smarthome::tasks::TaskSpawner;

const PIR_CFG: &str = r#"
[pir1]
_class = "WbPirSensor"
_mqtt_used = true
name = "Движение"
status_path = "pir"
"#;

fn build_pir() -> (BuiltDevices, TaskSpawner) {
    let table: toml::Table = toml::from_str(PIR_CFG).unwrap();
    let (handle, _rx) = mqtt::channel();
    let tasks = TaskSpawner::new();
    let built = devices::build_all(
        &table,
        &BuildContext {
            mqtt: Some(handle),
            tasks: tasks.clone(),
        },
    )
    .unwrap();
    (built, tasks)
}

fn motion_state(value: &str) -> Value {
    json!({
        "type": "devices.properties.event",
        "state": {"instance": "motion", "value": value},
    })
}

/// A pulse entirely between sampling snapshots must still produce both
/// transitions, carrying the values observed at the edges.
#[tokio::test]
async fn pir_pulse_produces_both_edges() {
    let (built, tasks) = build_pir();
    let (sink, mut events) = StateSink::channel();

    // WB PIR: '0' = detected, anything else = not_detected
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "0");
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "1");

    let first = events.try_recv().unwrap();
    assert_eq!(first.device_id, "pir1");
    assert_eq!(first.entries, vec![motion_state("detected")]);
    let second = events.try_recv().unwrap();
    assert_eq!(second.entries, vec![motion_state("not_detected")]);
    assert!(events.try_recv().is_err());

    // a repeat of the same value is not a transition
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "1");
    assert!(events.try_recv().is_err());
}

/// Capability-only updates do not use the immediate path.
#[tokio::test]
async fn capability_changes_not_pushed() {
    let cfg = r#"
[light1]
_class = "WbLight"
_mqtt_used = true
name = "x"
status_path = "s"
control_path = "s/on"
"#;
    let table: toml::Table = toml::from_str(cfg).unwrap();
    let (handle, _rx) = mqtt::channel();
    let tasks = TaskSpawner::new();
    let built = devices::build_all(
        &table,
        &BuildContext {
            mqtt: Some(handle),
            tasks: tasks.clone(),
        },
    )
    .unwrap();
    let (sink, mut events) = StateSink::channel();
    mqtt::dispatch(&built, &tasks, Some(&sink), "s", "1");
    assert!(events.try_recv().is_err());
}

#[test]
fn drain_batch_splits_conflicting_edges() {
    let mut pending = VecDeque::from([
        StateChange {
            device_id: "pir1".into(),
            entries: vec![motion_state("detected")],
        },
        StateChange {
            device_id: "leak1".into(),
            entries: vec![json!({
                "type": "devices.properties.event",
                "state": {"instance": "water_leak", "value": "leak"},
            })],
        },
        StateChange {
            device_id: "pir1".into(),
            entries: vec![motion_state("not_detected")],
        },
    ]);

    // the second pir1 transition conflicts and must wait for the next batch
    let batch = drain_batch(&mut pending);
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].entries, vec![motion_state("detected")]);
    assert_eq!(pending.len(), 1);
    let batch = drain_batch(&mut pending);
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].entries, vec![motion_state("not_detected")]);
}

/// A source chattering faster than the send gate must not grow the queue
/// without bound: past the cap the oldest transition goes, newest stays.
#[test]
fn enqueue_drops_oldest_past_cap() {
    let mut pending = VecDeque::new();
    for i in 0..MAX_PENDING {
        let dropped = enqueue(
            &mut pending,
            StateChange {
                device_id: format!("pir{i}"),
                entries: vec![motion_state("detected")],
            },
        );
        assert!(!dropped);
    }
    let dropped = enqueue(
        &mut pending,
        StateChange {
            device_id: "newest".into(),
            entries: vec![motion_state("not_detected")],
        },
    );
    assert!(dropped);
    assert_eq!(pending.len(), MAX_PENDING);
    assert_eq!(pending.front().unwrap().device_id, "pir1");
    assert_eq!(pending.back().unwrap().device_id, "newest");
}

#[test]
fn merge_reported_replaces_by_instance() {
    let mut previous = HashMap::new();
    let change = |value: &str| StateChange {
        device_id: "pir1".into(),
        entries: vec![motion_state(value)],
    };

    merge_reported(&mut previous, &change("detected"));
    assert_eq!(
        previous["pir1"]["properties"],
        json!([motion_state("detected")])
    );
    // replaced, not appended: appending would wedge report()'s membership diff
    merge_reported(&mut previous, &change("not_detected"));
    assert_eq!(
        previous["pir1"]["properties"],
        json!([motion_state("not_detected")])
    );
    merge_reported(&mut previous, &change("detected"));
    assert_eq!(
        previous["pir1"]["properties"],
        json!([motion_state("detected")])
    );
}

/// After an edge was pushed and merged, the periodic pass sees no diff.
#[tokio::test]
async fn full_pass_does_not_resend_merged_edges() {
    let (built, tasks) = build_pir();
    let (sink, mut events) = StateSink::channel();
    let mut previous = HashMap::new();

    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "0");
    let change = events.try_recv().unwrap();
    merge_reported(&mut previous, &change);

    let states = full_pass(&built, &mut previous);
    assert_eq!(states, Vec::<Value>::new());

    // and a later real transition is again a diff
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "1");
    let states = full_pass(&built, &mut previous);
    assert_eq!(states.len(), 1);
    assert_eq!(
        states[0]["properties"],
        json!([motion_state("not_detected")])
    );
}

/// Minimal local HTTP listener collecting callback/state payloads.
async fn state_collector() -> (String, Arc<Mutex<Vec<Value>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let sink = sink.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                loop {
                    let mut chunk = [0u8; 4096];
                    let Ok(n) = stream.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&buf[..pos]).to_string();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        while buf.len() < pos + 4 + length {
                            let Ok(n) = stream.read(&mut chunk).await else {
                                return;
                            };
                            if n == 0 {
                                return;
                            }
                            buf.extend_from_slice(&chunk[..n]);
                        }
                        let body = &buf[pos + 4..pos + 4 + length];
                        if headers.starts_with("POST") && headers.contains("/state") {
                            if let Ok(value) = serde_json::from_slice::<Value>(body) {
                                sink.lock().unwrap().push(value);
                            }
                        }
                        let response = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 20\r\n\r\n{\"request_id\": \"t\"}\n";
                        let _ = stream.write_all(response).await;
                        return;
                    }
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), received)
}

/// End-to-end driver: an edge is delivered promptly without waiting for the
/// sampling tick, bursts share the gate, and the next tick does not resend.
#[tokio::test]
async fn driver_delivers_edges_promptly() {
    let (base_url, received) = state_collector().await;
    let (built, tasks) = build_pir();
    let (sink, events) = StateSink::channel();

    let cfg = NotificationsConfig {
        skill_id: "skill".into(),
        user_id: "user".into(),
        oauth_token: "token".into(),
    };
    let mut notifications = Notifications::with_base_url(cfg, base_url).unwrap();
    notifications.tick_period = Duration::from_millis(500);
    notifications.min_send_interval = Duration::from_millis(100);

    let built = Arc::new(built);
    let loop_devices = built.clone();
    let notifications = Arc::new(notifications);
    let driver = notifications.clone();
    tokio::spawn(async move { driver.notifications_loop(loop_devices, events).await });
    // let the loop start; the first sampling pass is one period away
    tokio::time::sleep(Duration::from_millis(50)).await;

    // pulse: both edges arrive back to back
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "0");
    mqtt::dispatch(&built, &tasks, Some(&sink), "pir", "1");

    // the first edge goes out immediately; the second waits out the gate
    tokio::time::sleep(Duration::from_millis(300)).await;
    {
        let posts = received.lock().unwrap();
        assert_eq!(posts.len(), 2, "{posts:?}");
        assert_eq!(
            posts[0]["payload"]["devices"][0]["properties"],
            json!([motion_state("detected")])
        );
        assert_eq!(
            posts[1]["payload"]["devices"][0]["properties"],
            json!([motion_state("not_detected")])
        );
    }

    // the sampling tick after the pulse has nothing new to say
    tokio::time::sleep(Duration::from_millis(700)).await;
    let posts = received.lock().unwrap();
    assert_eq!(posts.len(), 2, "sampling pass resent state: {posts:?}");
}
