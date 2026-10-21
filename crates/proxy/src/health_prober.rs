use crate::bounded_queue::BoundedQueue;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const DATA_RECORD_LENGTH: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct HealthDataPoint {
    pub id: String,
    pub ingress_addresses: Vec<String>,
    pub egress_addresses: Vec<String>,
    pub bytes_sent: u64,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Default)]
pub struct InstanceCounters {
    bytes_sent: AtomicU64,
    active_connections: AtomicU64,
    total_connections: AtomicU64,
}

impl InstanceCounters {
    pub fn add_bytes_sent(&self, n: u64) {
        self.bytes_sent.fetch_add(n, Ordering::Relaxed);
    }

    pub fn open_connection(self: &Arc<Self>) -> ConnectionGuard {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        ConnectionGuard(self.clone())
    }
}

pub struct ConnectionGuard(Arc<InstanceCounters>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceSummary {
    pub id: String,
    pub total_bytes_sent: u64,
    pub active_connections: u64,
    pub total_connections: u64,
}

#[derive(Default)]
pub struct HealthProber {
    counters: Mutex<HashMap<String, Arc<InstanceCounters>>>,
    data_points: Mutex<HashMap<String, BoundedQueue<HealthDataPoint>>>,
}

impl HealthProber {
    pub fn new() -> HealthProber {
        HealthProber::default()
    }

    pub fn counters(&self, id: &str) -> Arc<InstanceCounters> {
        self.counters
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    pub fn insert(&self, data_point: HealthDataPoint) {
        let mut data_points = self.data_points.lock().unwrap();
        data_points
            .entry(data_point.id.clone())
            .or_insert_with(|| BoundedQueue::new(DATA_RECORD_LENGTH))
            .push(data_point);
    }

    pub fn get_points(&self, id: &str) -> Option<Vec<HealthDataPoint>> {
        self.with_queue(id, |queue| queue.map(|q| q.iter().cloned().collect()))
    }

    pub fn summary(&self) -> Vec<InstanceSummary> {
        let counters = self.counters.lock().unwrap();
        let mut summary: Vec<InstanceSummary> = counters
            .iter()
            .map(|(id, c)| InstanceSummary {
                id: id.clone(),
                total_bytes_sent: c.bytes_sent.load(Ordering::Relaxed),
                active_connections: c.active_connections.load(Ordering::Relaxed),
                total_connections: c.total_connections.load(Ordering::Relaxed),
            })
            .collect();
        summary.sort_by(|a, b| a.id.cmp(&b.id));
        summary
    }

    pub fn total_bytes_sent(&self) -> u64 {
        self.counters
            .lock()
            .unwrap()
            .values()
            .map(|c| c.bytes_sent.load(Ordering::Relaxed))
            .sum()
    }

    pub fn with_queue<R>(
        &self,
        id: &str,
        f: impl FnOnce(Option<&BoundedQueue<HealthDataPoint>>) -> R,
    ) -> R {
        let data_points = self.data_points.lock().unwrap();
        f(data_points.get(id))
    }
}
