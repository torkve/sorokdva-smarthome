use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, SubscribeFilter};
use serde::Deserialize;

use crate::devices::{self, BuiltDevices, Device, StateSink};
use crate::tasks::TaskSpawner;

#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub login: String,
    #[serde(default)]
    pub password: Option<String>,
    /// MQTT client id. Must be unique per broker connection: a second
    /// client connecting with the same id kicks the first one off
    /// (client-id takeover).
    #[serde(default = "default_client_id")]
    pub client_id: String,
}

fn default_host() -> String {
    "localhost".to_string()
}

fn default_port() -> u16 {
    1883
}

fn default_client_id() -> String {
    "sorokdva-dialogs-rs".to_string()
}

/// Commands waiting for the broker. While it is unreachable nothing is
/// delivered, and replaying hours of stale commands on reconnect would be
/// wrong anyway, so past the cap the oldest command is dropped.
const MAX_OUTGOING: usize = 64;

struct Outgoing {
    queue: std::sync::Mutex<VecDeque<(String, String)>>,
    notify: tokio::sync::Notify,
}

/// Fire-and-forget publish handle used by device logic.
#[derive(Clone)]
pub struct MqttHandle(Arc<Outgoing>);

impl MqttHandle {
    pub fn send(&self, topic: &str, message: impl Into<String>) {
        {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            if queue.len() >= MAX_OUTGOING {
                if let Some((dropped, _)) = queue.pop_front() {
                    warn!(target: "mqtt", "broker unreachable, dropping the oldest queued command ({dropped:?})");
                }
            }
            queue.push_back((topic.to_string(), message.into()));
        }
        self.0.notify.notify_one();
    }
}

/// The consumer side of the command queue.
pub struct OutgoingReceiver(Arc<Outgoing>);

impl OutgoingReceiver {
    pub fn try_recv(&mut self) -> Option<(String, String)> {
        self.0
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    pub async fn recv(&mut self) -> (String, String) {
        loop {
            if let Some(command) = self.try_recv() {
                return command;
            }
            self.0.notify.notified().await;
        }
    }
}

pub fn channel() -> (MqttHandle, OutgoingReceiver) {
    let outgoing = Arc::new(Outgoing {
        queue: std::sync::Mutex::new(VecDeque::new()),
        notify: tokio::sync::Notify::new(),
    });
    (MqttHandle(outgoing.clone()), OutgoingReceiver(outgoing))
}

pub fn dispatch(
    devices: &BuiltDevices,
    tasks: &TaskSpawner,
    sink: Option<&StateSink>,
    topic: &str,
    payload: &str,
) {
    let Some(indices) = devices.subscriptions.get(topic) else {
        return;
    };
    for &index in indices {
        let Some((device_id, device)) = devices.devices.get(index) else {
            continue;
        };
        debug!(target: "mqtt", "passing ({topic:?}, {payload:?}) to {device_id}");
        let settle = devices::with_notify(device_id, device, sink, |dev| {
            let Device { core, logic } = dev;
            logic.on_mqtt(core, topic, payload)
        });
        if let Some(delay) = settle {
            // The key must stay per-device and constant: spawn_replacing keeps
            // one slot per key forever, and replacing is what coalesces a
            // burst of channel updates into a single settle.
            let device = device.clone();
            let device_id = device_id.clone();
            let sink = sink.cloned();
            tasks.spawn_replacing(&format!("settle:{device_id}"), async move {
                tokio::time::sleep(delay).await;
                // no await below: once started, the mutate-compare-emit
                // region runs to completion — an abort can only land on the
                // sleep above, never between a mutation and its notification
                devices::with_notify(&device_id, &device, sink.as_ref(), |dev| {
                    let Device { core, logic } = dev;
                    logic.on_settle(core);
                });
            });
        }
    }
}

/// Run the MQTT client: maintain the connection, resubscribe on connect,
/// forward outgoing publishes, ping every 10s, dispatch incoming messages.
pub async fn run(
    cfg: MqttConfig,
    mut outgoing: OutgoingReceiver,
    devices: Arc<BuiltDevices>,
    tasks: TaskSpawner,
    sink: Option<StateSink>,
) {
    let mut options = MqttOptions::new(cfg.client_id.clone(), cfg.host.clone(), cfg.port);
    options.set_keep_alive(Duration::from_secs(30));
    // a retained message larger than the incoming limit fails every
    // connect; the default 10 KiB is too small for /meta blobs on the bus
    options.set_max_packet_size(256 * 1024, 10 * 1024);
    if !cfg.login.is_empty() || cfg.password.is_some() {
        options.set_credentials(cfg.login.clone(), cfg.password.clone().unwrap_or_default());
    }

    let (client, mut eventloop) = AsyncClient::new(options, 64);

    // outgoing publishes (device actions)
    let publisher = client.clone();
    tokio::spawn(async move {
        loop {
            let (topic, message) = outgoing.recv().await;
            let _ = publisher
                .publish(topic, QoS::AtMostOnce, false, message)
                .await;
        }
    });

    // application-level heartbeat: publish "ping" on the `smarthome`
    // topic every 10 seconds (distinct from the MQTT keepalive above)
    let pinger = client.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        // ticks missed while the publish blocks on a broker outage are
        // skipped, not replayed as a burst afterwards
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let _ = pinger
                .publish("smarthome", QoS::AtMostOnce, false, "ping")
                .await;
        }
    });

    // only the topics devices actually read: the broker carries the whole
    // bus, and receiving all of it would cost bandwidth and CPU for nothing
    let filters: Vec<SubscribeFilter> = devices
        .subscriptions
        .keys()
        .map(|topic| SubscribeFilter::new(topic.clone(), QoS::AtMostOnce))
        .collect();

    // reconnect backoff: 1s doubling to 60s, reset by a successful connect;
    // the warning is repeated only every 10th attempt once it stalls
    let mut backoff = Duration::from_secs(1);
    let mut failures: u32 = 0;
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                info!(target: "mqtt", "connected to {}:{}", cfg.host, cfg.port);
                backoff = Duration::from_secs(1);
                failures = 0;
                // spawned: awaiting here would stop driving the event loop
                // that the request channel needs drained
                if !filters.is_empty() {
                    let client = client.clone();
                    let filters = filters.clone();
                    tokio::spawn(async move {
                        let _ = client.subscribe_many(filters).await;
                    });
                }
            }
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                match std::str::from_utf8(&publish.payload) {
                    Ok(payload) => {
                        dispatch(&devices, &tasks, sink.as_ref(), &publish.topic, payload)
                    }
                    Err(_) => {
                        warn!(target: "mqtt", "non-utf8 payload on {}", publish.topic);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                failures = failures.saturating_add(1);
                if failures <= 3 || failures.is_multiple_of(10) {
                    warn!(
                        target: "mqtt",
                        "connection error: {e}; reconnecting in {}s (attempt {failures})",
                        backoff.as_secs()
                    );
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn outgoing_queue_drops_oldest_past_cap() {
        let (handle, mut rx) = channel();
        for i in 0..(MAX_OUTGOING + 5) {
            handle.send("t", i.to_string());
        }
        let mut got = Vec::new();
        while let Some((_, message)) = rx.try_recv() {
            got.push(message);
        }
        assert_eq!(got.len(), MAX_OUTGOING);
        assert_eq!(got.first().map(String::as_str), Some("5"));
        assert_eq!(got.last(), Some(&(MAX_OUTGOING + 4).to_string()));

        // recv wakes up for a command sent after it started waiting
        let waiter = tokio::spawn(async move { rx.recv().await });
        tokio::task::yield_now().await;
        handle.send("late", "1");
        assert_eq!(waiter.await.unwrap(), ("late".to_string(), "1".to_string()));
    }

    #[test]
    fn config_defaults() {
        let cfg: MqttConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 1883);
        assert_eq!(cfg.client_id, "sorokdva-dialogs-rs");

        let cfg: MqttConfig = toml::from_str("host = \"broker\"\nclient_id = \"custom\"").unwrap();
        assert_eq!(cfg.host, "broker");
        assert_eq!(cfg.client_id, "custom");
    }
}
