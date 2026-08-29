use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use tokio::task::AbortHandle;

/// Keyed fire-and-forget task slots: spawning under a key aborts the
/// previously running task with the same key, so per-device timers
/// (curtain auto-stop, settle windows) never stack.
#[derive(Clone, Default)]
pub struct TaskSpawner(Arc<Mutex<HashMap<String, AbortHandle>>>);

impl TaskSpawner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn_replacing<F>(&self, key: &str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slots.remove(key) {
            previous.abort();
        }
        slots.insert(key.to_string(), tokio::spawn(future).abort_handle());
    }
}
