use chrono::{DateTime, Utc};
use serde::Serialize;

use super::types::{MqttPublish, Qos};

pub trait CommandKind {
    fn kind(&self) -> &'static str;
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandEnvelope<T: Serialize> {
    pub kind: String,
    pub ts: DateTime<Utc>,
    pub data: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetMachine {
    pub machine_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetConveyorSpeed {
    pub conveyor_id: String, // e.g., "förderband1-4"
    pub speed: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Command {
    ResetMachine(ResetMachine),
    SetConveyorSpeed(SetConveyorSpeed),
}

impl CommandKind for Command {
    fn kind(&self) -> &'static str {
        match self {
            Command::ResetMachine(_) => "reset_machine",
            Command::SetConveyorSpeed(_) => "set_conveyor_speed",
        }
    }
}

impl Command {
    /// Encode to MQTT publish (topic mapping belongs here).
    /// You can change these topics freely without touching gateway/downstream.
    pub fn to_mqtt(&self, command_prefix: &str) -> anyhow::Result<MqttPublish> {
        let kind = self.kind().to_string();
        let ts = Utc::now();

        let (topic_suffix, payload) = match self {
            Command::ResetMachine(cmd) => {
                let env = CommandEnvelope { kind, ts, data: cmd };
                ("reset_machine", serde_json::to_vec(&env)?)
            }
            Command::SetConveyorSpeed(cmd) => {
                let env = CommandEnvelope { kind, ts, data: cmd };
                ("set_conveyor_speed", serde_json::to_vec(&env)?)
            }
        };

        Ok(MqttPublish {
            topic: format!("{}/{}", command_prefix.trim_end_matches('/'), topic_suffix),
            payload,
            retain: false,
            qos: Qos::AtLeastOnce,
        })
    }
}
