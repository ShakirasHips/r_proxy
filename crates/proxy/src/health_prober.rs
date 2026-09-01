use crate::bounded_queue::BoundedQueue;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use serde::Serialize;

const DATA_RECORD_LENGTH: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct HealthDataPoint {
    pub id: String,
    pub ingress_addresses: Vec<String>,
    pub egress_addresses: Vec<String>,
    pub bytes_sent: u32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Default)]
pub struct HealthProber {
    data_points: Mutex<HashMap<String, BoundedQueue<HealthDataPoint>>>,
}

impl HealthProber {
    pub fn new() -> HealthProber {
        HealthProber {
            data_points:  Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, data_point: HealthDataPoint) {
        let mut data_points = self.data_points.lock().unwrap();
        data_points
            .entry(data_point.id.clone())
            .or_insert_with(|| BoundedQueue::new(DATA_RECORD_LENGTH))
            .push(data_point);
    }

    pub fn print(&self) {
        let data_points = self.data_points.lock().unwrap();
        for (id, queue) in data_points.iter() {
            let total: u64 = queue.iter().map(|dp| dp.bytes_sent as u64).sum();
            println!("{}: {}", id, total);
        }
    }

    pub fn get_points(&self, id: &str) -> Option<Vec<HealthDataPoint>> {
        self.with_queue(id, |queue| {
            queue.map(|q| q.iter().cloned().collect())
        })
    }

    pub fn summary(&self) -> Vec<(String, u64)> {
        let data_points = self.data_points.lock().unwrap();
        data_points
            .iter()
            .map(|(id, q)| (id.clone(), q.iter().map(|dp| dp.bytes_sent as u64).sum()))
            .collect()
    }

    pub fn with_queue<R>(&self, id: &str, f: impl FnOnce(Option<&BoundedQueue<HealthDataPoint>>) -> R) -> R {
        let data_points = self.data_points.lock().unwrap();
        f(data_points.get(id))
    }
}
