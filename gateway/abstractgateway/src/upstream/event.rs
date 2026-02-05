use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;

pub type Id = String;
pub type AnyValue = Value;

/// A lightweight event *envelope* (data), not an event *mechanism*.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: Id,                       // e.g. "machine1/overheat"
    pub ty: String,                   // e.g. "OverheatDetected"
    pub timestamp: SystemTime,
    pub payload: AnyValue,            // protocol-agnostic payload
    pub attributes: HashMap<String, AnyValue>, // optional metadata (trace_id, severity, etc.)
}
