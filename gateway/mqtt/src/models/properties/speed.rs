use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::models::commands::{Command, SetConveyorSpeed};
use crate::models::events::{TelemetryEvent, Value};

#[derive(Debug, Clone)]
pub struct SpeedState {
    pub value: f64,
    pub unit: Option<String>,
    pub ts: DateTime<Utc>,
}

/// Tracks last-known speed per asset id (e.g., conveyor id).
#[derive(Debug, Default)]
pub struct SpeedProperty {
    values: HashMap<String, SpeedState>,
}

impl SpeedProperty {
    /// Engine-facing getter
    pub fn get(&self, asset_id: &str) -> Option<&SpeedState> {
        self.values.get(asset_id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }

    /// Engine-facing "setter": expresses intent and returns a command to send.
    pub fn set_target_speed_cmd(&self, conveyor_id: impl Into<String>, speed: f64) -> Command {
        Command::SetConveyorSpeed(SetConveyorSpeed {
            conveyor_id: conveyor_id.into(),
            speed,
        })
    }

    /// Gateway-facing update from telemetry
    pub fn update_from_telemetry(&mut self, t: &TelemetryEvent) {
        // Convention: "<asset>/speed"
        let (asset_id, is_speed_topic) = split_asset_and_prop(&t.topic);
        if !is_speed_topic {
            return;
        }

        let (v, unit) = match &t.value {
            Value::Number { v, unit } => (*v, unit.clone()),
            _ => return,
        };

        self.values.insert(
            asset_id,
            SpeedState {
                value: v,
                unit,
                ts: t.ts,
            },
        );
    }
}

fn split_asset_and_prop(topic: &str) -> (String, bool) {
    let topic = topic.trim();

    if let Some((asset, prop)) = topic.rsplit_once('/') {
        let prop_l = prop.to_ascii_lowercase();
        let matches = prop_l == "speed" || prop_l == "rpm";
        return (asset.to_string(), matches);
    }

    (topic.to_string(), false)
}
