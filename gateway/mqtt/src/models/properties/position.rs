use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::models::events::{TelemetryEvent, Value};

#[derive(Debug, Clone)]
pub struct PositionState {
    pub value: f64,
    pub unit: Option<String>,
    pub ts: DateTime<Utc>,
}

/// Tracks last-known position per asset id (e.g., conveyor id).
#[derive(Debug, Default)]
pub struct PositionProperty {
    // key = asset id, e.g. "förderband1-4"
    values: HashMap<String, PositionState>,
}

impl PositionProperty {
    /// Engine-facing getter (read-only)
    pub fn get(&self, asset_id: &str) -> Option<&PositionState> {
        self.values.get(asset_id)
    }

    /// Engine-facing getter: list all known ids
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }

    /// For tests/simulation: manually set the position
    pub fn set_manual(&mut self, asset_id: impl Into<String>, value: f64, unit: Option<String>) {
        self.values.insert(
            asset_id.into(),
            PositionState {
                value,
                unit,
                ts: Utc::now(),
            },
        );
    }

    /// Gateway-facing update from telemetry
    pub fn update_from_telemetry(&mut self, t: &TelemetryEvent) {
        // Convention: "<asset>/position" or "<asset>/pos"
        let (asset_id, is_position_topic) = split_asset_and_prop(&t.topic);
        if !is_position_topic {
            return;
        }

        let (v, unit) = match &t.value {
            Value::Number { v, unit } => (*v, unit.clone()),
            _ => return, // ignore non-numeric
        };

        self.values.insert(
            asset_id,
            PositionState {
                value: v,
                unit,
                ts: t.ts,
            },
        );
    }
}

/// Returns (asset_id, matchesPosition)
fn split_asset_and_prop(topic: &str) -> (String, bool) {
    let topic = topic.trim();

    if let Some((asset, prop)) = topic.rsplit_once('/') {
        let prop_l = prop.to_ascii_lowercase();
        let matches = prop_l == "position" || prop_l == "pos";
        return (asset.to_string(), matches);
    }

    // If there is no '/' segment, we do NOT assume it's a position.
    (topic.to_string(), false)
}
