use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::models::types::Severity;

#[derive(Debug, Clone, Serialize)]
pub enum Value {
    Number { v: f64, unit: Option<String> },
    Text(String),
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryEvent {
    pub topic: String,
    pub data_raw: String,
    pub value: Value,
    pub ts: DateTime<Utc>,
    pub source_mqtt_topic: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineFailureEvent {
    pub machine_id: String,
    pub component: String,
    pub failure_code: Option<String>,
    pub severity: Severity,
    pub message: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CounterDeviationEvent {
    pub machine_id: String,
    pub counter_name: String,
    pub expected: i64,
    pub actual: i64,
    pub tolerance: Option<i64>,
    pub severity: Severity,
    pub message: Option<String>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum GatewayEvent {
    Telemetry(TelemetryEvent),
    MachineFailure(MachineFailureEvent),
    CounterDeviation(CounterDeviationEvent),
}
