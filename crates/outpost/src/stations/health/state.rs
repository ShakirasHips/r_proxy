// use crate::models::health::HealthEntry;
use std::sync::{atomic::AtomicU64, Mutex};

#[derive(Default)]
pub struct HealthState {
    pub entries: Mutex<Vec<i32>>,
    pub next_id: AtomicU64,
}