use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// ---- Wire schema (events on MQTT) ----

pub const EVENT_SCHEMA_V1: &str = "ft.events.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
}

/// A versioned envelope for structured events.
/// Payloads can be either a single `EventEnvelope` or a JSON array of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Must be "ft.events.v1" for this version.
    pub schema: String,

    /// Optional event id (uuid recommended)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Event timestamp in RFC3339
    pub ts: DateTime<Utc>,

    pub source: SourceInfo,

    pub event: WireEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum WireEvent {
    #[serde(rename = "machine_failure")]
    MachineFailure(MachineFailureData),

    #[serde(rename = "counter_deviation")]
    CounterDeviation(CounterDeviationData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineFailureData {
    pub component: String,

    /// Example: "stuck", "overheat", "timeout"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,

    #[serde(default)]
    pub severity: Severity,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterDeviationData {
    /// Logical name of the counter
    pub counter_name: String,

    pub expected: i64,
    pub actual: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<i64>,

    #[serde(default)]
    pub severity: Severity,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Default for Severity {
    fn default() -> Self {
        Severity::Info
    }
}

/// Helper: payload may be a single object or an array.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WireEventPayload {
    One(EventEnvelope),
    Many(Vec<EventEnvelope>),
}

impl WireEventPayload {
    pub fn into_vec(self) -> Vec<EventEnvelope> {
        match self {
            WireEventPayload::One(e) => vec![e],
            WireEventPayload::Many(v) => v,
        }
    }
}

/// ---- Transport-level MQTT types ----

#[derive(Debug, Clone)]
pub struct MqttMessage {
    pub topic: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Qos {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

#[derive(Debug, Clone)]
pub struct MqttPublish {
    pub topic: String,
    pub payload: Vec<u8>,
    pub retain: bool,
    pub qos: Qos,
}

