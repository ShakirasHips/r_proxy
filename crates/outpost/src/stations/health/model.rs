use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEntry {
    pub id: u64,
    pub metric_type: String,
    pub value: f64,
    pub unit: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateHealthEntry {
    pub metric_type: String,
    pub value: f64,
    pub unit: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct HealthQuery {
    pub metric_type: Option<String>,
}