//! Push notifications to the Yandex Dialogs callback API: a periodic
//! pass sampling and diffing every device's state, plus immediate
//! delivery of event transitions (PIR motion, leaks) whose pulses can be
//! shorter than the sampling period.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use http::{header, HeaderValue};
use log::{info, warn};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::config::NotificationsConfig;
use crate::devices::{lock, BuiltDevices, StateChange};
use crate::httpc::HttpClient;

pub struct Notifications {
    cfg: NotificationsConfig,
    client: HttpClient,
    base_url: String,
    /// Period of the full sampling pass; the first pass runs one period
    /// after startup.
    pub tick_period: Duration,
    /// Minimum interval between callback/state POSTs. This single gate
    /// serves as the edge coalescer, the rate limiter and the
    /// reconnect-flood cap: the first edge after a quiet spell goes out
    /// immediately, bursts collapse into one POST per interval.
    pub min_send_interval: Duration,
}

fn ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.)
}

/// The periodic sampling pass: diff every device's reportable state against
/// what was last reported, update `previous`, and return the payloads to
/// send. `previous` holds, per device, the last values reported to Yandex.
pub fn full_pass(devices: &BuiltDevices, previous: &mut HashMap<String, Value>) -> Vec<Value> {
    let mut states: Vec<Value> = Vec::new();
    for (device_id, device) in &devices.devices {
        let empty = json!({});
        let prev = previous.get(device_id).unwrap_or(&empty);
        let report = lock(device).core.report(prev);

        let collect = |key: &str, changed_only: bool| -> Vec<Value> {
            report
                .get(key)
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| {
                            let state = entry.get(0)?;
                            let changed = entry.get(1).and_then(Value::as_bool).unwrap_or(false);
                            if !changed_only || changed {
                                Some(state.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let changed_capabilities = collect("capabilities", true);
        let changed_properties = collect("properties", true);
        if !changed_capabilities.is_empty() || !changed_properties.is_empty() {
            states.push(json!({
                "id": device_id,
                "capabilities": changed_capabilities,
                "properties": changed_properties,
            }));
        }
        previous.insert(
            device_id.clone(),
            json!({
                "id": device_id,
                "capabilities": collect("capabilities", false),
                "properties": collect("properties", false),
            }),
        );
    }
    states
}

/// Bound on queued-but-unsent transitions. A source chattering faster than
/// the send gate (a bouncing contact, a corroded input) would otherwise grow
/// the queue without limit; past the cap the oldest transition is dropped —
/// the periodic pass re-synchronises the final state within one period.
pub const MAX_PENDING: usize = 64;

/// Queue a transition, dropping the oldest one when the queue is full.
/// Returns whether something was dropped.
pub fn enqueue(pending: &mut VecDeque<StateChange>, change: StateChange) -> bool {
    let dropped = pending.len() >= MAX_PENDING;
    if dropped {
        pending.pop_front();
    }
    pending.push_back(change);
    dropped
}

fn entry_key(entry: &Value) -> (String, String) {
    (
        entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        entry
            .pointer("/state/instance")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    )
}

fn entry_list(entry: &Value) -> &'static str {
    if entry
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.starts_with("devices.capabilities."))
    {
        "capabilities"
    } else {
        "properties"
    }
}

/// Drain queued edges into one batch of per-device payloads. Draining stops
/// at the first entry whose (device, type, instance) already appears in the
/// batch: a later transition of the same instance must go into a later POST
/// (collapsing detected -> not_detected into one sample is the very bug
/// this path exists to fix). The remainder stays queued.
pub fn drain_batch(pending: &mut VecDeque<StateChange>) -> Vec<StateChange> {
    let mut batch: Vec<StateChange> = Vec::new();
    let mut seen: Vec<(String, String, String)> = Vec::new();
    while let Some(change) = pending.front() {
        let conflicts = change.entries.iter().any(|entry| {
            let (t, i) = entry_key(entry);
            seen.contains(&(change.device_id.clone(), t, i))
        });
        if conflicts {
            break;
        }
        let change = pending.pop_front().expect("front checked above");
        for entry in &change.entries {
            let (t, i) = entry_key(entry);
            seen.push((change.device_id.clone(), t, i));
        }
        batch.push(change);
    }
    batch
}

/// Merge sent edge entries into `previous` ("last reported"), replacing the
/// entry with the same (type, instance). Overwriting the whole snapshot
/// would drop a concurrent falling edge; appending would wedge the diffing
/// (report() uses list membership).
pub fn merge_reported(previous: &mut HashMap<String, Value>, change: &StateChange) {
    let snapshot = previous
        .entry(change.device_id.clone())
        .or_insert_with(|| json!({"id": change.device_id, "capabilities": [], "properties": []}));
    for entry in &change.entries {
        let key = entry_key(entry);
        let Some(list) = snapshot
            .get_mut(entry_list(entry))
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        match list.iter_mut().find(|e| entry_key(e) == key) {
            Some(existing) => *existing = entry.clone(),
            None => list.push(entry.clone()),
        }
    }
}

/// Group a batch of changes into the devices payload for callback/state.
pub fn batch_payload(batch: &[StateChange]) -> Vec<Value> {
    let mut devices: Vec<Value> = Vec::new();
    for change in batch {
        let mut capabilities: Vec<Value> = Vec::new();
        let mut properties: Vec<Value> = Vec::new();
        for entry in &change.entries {
            match entry_list(entry) {
                "capabilities" => capabilities.push(entry.clone()),
                _ => properties.push(entry.clone()),
            }
        }
        devices.push(json!({
            "id": change.device_id,
            "capabilities": capabilities,
            "properties": properties,
        }));
    }
    devices
}

impl Notifications {
    pub fn new(cfg: NotificationsConfig) -> Result<Self> {
        let base_url = format!(
            "https://dialogs.yandex.net/api/v1/skills/{}/callback",
            cfg.skill_id
        );
        Self::with_base_url(cfg, base_url)
    }

    /// Constructor with an explicit callback base URL (tests point it at a
    /// local plain-http listener).
    pub fn with_base_url(cfg: NotificationsConfig, base_url: String) -> Result<Self> {
        let auth = HeaderValue::from_str(&format!("OAuth {}", cfg.oauth_token))
            .context("invalid oauth token")?;
        let client = HttpClient::new(
            Duration::from_secs(2),
            Duration::from_secs(30),
            vec![(header::AUTHORIZATION, auth)],
        );
        Ok(Notifications {
            cfg,
            client,
            base_url,
            tick_period: Duration::from_secs(10),
            min_send_interval: Duration::from_secs(1),
        })
    }

    async fn post(&self, endpoint: &str, payload: Value) -> Result<Value> {
        let url = format!("{}/{}", self.base_url, endpoint);
        let (status, data) = self
            .client
            .post_json(&url, &payload)
            .await
            .with_context(|| format!("cannot POST {url}"))?;
        if status.is_success() {
            Ok(data)
        } else {
            bail!("notification failed: {status}, {data}, url: {url}");
        }
    }

    /// send_device_specifications_updated: POST callback/discovery.
    pub async fn send_discovery(&self) -> Result<()> {
        let data = self
            .post(
                "discovery",
                json!({
                    "ts": ts(),
                    "payload": {"user_id": self.cfg.user_id},
                }),
            )
            .await?;
        info!(
            target: "notifications",
            "Sent device specs updated, request_id={:?}",
            data.get("request_id").and_then(Value::as_str).unwrap_or(""),
        );
        Ok(())
    }

    async fn send_device_states(&self, devices: Vec<Value>) -> Result<()> {
        let data = self
            .post(
                "state",
                json!({
                    "ts": ts(),
                    "payload": {
                        "user_id": self.cfg.user_id,
                        "devices": devices,
                    },
                }),
            )
            .await?;
        info!(
            target: "notifications",
            "Sent state, request_id={:?}",
            data.get("request_id").and_then(Value::as_str).unwrap_or(""),
        );
        Ok(())
    }

    /// The notification driver: a periodic sampling pass (the first one,
    /// a period after startup, reports everything) plus immediate delivery
    /// of event transitions arriving on `events`. Everything shares one
    /// send gate; edges flush before a due pass so the pass cannot mask
    /// them, but a pass deferred for a full extra period runs anyway so a
    /// flapping source cannot starve sampling.
    pub async fn notifications_loop(
        &self,
        devices: Arc<BuiltDevices>,
        events: UnboundedReceiver<StateChange>,
    ) {
        use tokio::sync::mpsc::error::TryRecvError;

        let mut previous: HashMap<String, Value> = HashMap::new();
        let mut pending: VecDeque<StateChange> = VecDeque::new();
        let mut events = Some(events);
        let mut ticks_due: u32 = 0;
        let mut dropped: u64 = 0;
        let mut last_send: Option<tokio::time::Instant> = None;

        let start = tokio::time::Instant::now() + self.tick_period;
        let mut ticker = tokio::time::interval_at(start, self.tick_period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            // Pull everything already sitting in the channel before choosing
            // what to send: an edge left in the channel while a due pass runs
            // would be re-sent right after the pass reported the same value.
            if let Some(rx) = events.as_mut() {
                loop {
                    match rx.try_recv() {
                        Ok(change) => {
                            if enqueue(&mut pending, change) {
                                dropped += 1;
                                if dropped == 1 {
                                    warn!(
                                        target: "notifications",
                                        "event queue full, dropping oldest transitions",
                                    );
                                }
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            events = None;
                            break;
                        }
                    }
                }
            }

            let gate_at = last_send.map(|t| t + self.min_send_interval);
            let gate_open = gate_at
                .map(|g| tokio::time::Instant::now() >= g)
                .unwrap_or(true);
            let work = !pending.is_empty() || ticks_due > 0;

            if !work || !gate_open {
                let recv = async {
                    match events.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    _ = ticker.tick() => {
                        ticks_due = ticks_due.saturating_add(1);
                    }
                    change = recv => match change {
                        Some(change) => {
                            if enqueue(&mut pending, change) {
                                dropped += 1;
                                if dropped == 1 {
                                    warn!(
                                        target: "notifications",
                                        "event queue full, dropping oldest transitions",
                                    );
                                }
                            }
                        }
                        None => events = None,
                    },
                    _ = tokio::time::sleep_until(gate_at.unwrap_or_else(tokio::time::Instant::now)),
                        if work && !gate_open => {}
                }
                continue;
            }

            // Fairness: ticks_due >= 2 means a whole extra period passed while
            // edges kept the queue busy — sample anyway.
            if !pending.is_empty() && ticks_due < 2 {
                let batch = drain_batch(&mut pending);
                if !batch.is_empty() {
                    last_send = Some(tokio::time::Instant::now());
                    if let Err(e) = self.send_device_states(batch_payload(&batch)).await {
                        warn!(target: "notifications", "{e}");
                    }
                    for change in &batch {
                        merge_reported(&mut previous, change);
                    }
                }
                if pending.is_empty() && dropped > 0 {
                    warn!(
                        target: "notifications",
                        "event queue drained, {dropped} transitions were dropped",
                    );
                    dropped = 0;
                }
                continue;
            }

            ticks_due = 0;
            let states = full_pass(&devices, &mut previous);
            if !states.is_empty() {
                last_send = Some(tokio::time::Instant::now());
                if let Err(e) = self.send_device_states(states).await {
                    warn!(target: "notifications", "{e}");
                }
            }
        }
    }
}
