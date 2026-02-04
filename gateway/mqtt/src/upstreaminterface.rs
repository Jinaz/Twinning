use anyhow::Result;

use crate::models::{
    commands::Command,
    events::{CounterDeviationEvent, GatewayEvent, MachineFailureEvent, TelemetryEvent, Value},
    parser,
    types::{EventEnvelope, WireEvent, WireEventPayload, EVENT_SCHEMA_V1, MqttMessage, MqttPublish},
};

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    pub command_prefix: String,
    pub structured_event_prefix: Option<String>,
}

pub trait UpstreamInterface: Send + Sync {
    fn decode_events(&self, msg: &MqttMessage) -> Vec<GatewayEvent>;
    fn encode_command(&self, cmd: &Command) -> Result<MqttPublish>;
}

#[derive(Debug, Clone)]
pub struct DefaultUpstream {
    cfg: UpstreamConfig,
}

impl DefaultUpstream {
    pub fn new(cfg: UpstreamConfig) -> Self {
        Self { cfg }
    }

    fn should_try_structured(&self, topic: &str, payload: &str) -> bool {
        if let Some(prefix) = &self.cfg.structured_event_prefix {
            return topic.starts_with(prefix);
        }
        // If no prefix configured, only try when payload looks like it might be an envelope
        payload.contains(r#""schema""#) && payload.contains(r#""event""#) && payload.contains(r#""kind""#)
    }

    fn decode_structured(&self, msg: &MqttMessage, payload: &str) -> Option<Vec<GatewayEvent>> {
        if !self.should_try_structured(&msg.topic, payload) {
            return None;
        }

        let wire: WireEventPayload = serde_json::from_str(payload).ok()?;
        let mut out = Vec::new();

        for env in wire.into_vec() {
            // hard schema check
            if env.schema != EVENT_SCHEMA_V1 {
                continue;
            }
            out.push(self.map_envelope_to_event(env));
        }

        if out.is_empty() { None } else { Some(out) }
    }

    fn map_envelope_to_event(&self, env: EventEnvelope) -> GatewayEvent {
        match env.event {
            WireEvent::MachineFailure(d) => GatewayEvent::MachineFailure(MachineFailureEvent {
                machine_id: env.source.machine_id,
                component: d.component,
                failure_code: d.failure_code,
                severity: d.severity,
                message: d.message,
                ts: env.ts,
            }),
            WireEvent::CounterDeviation(d) => GatewayEvent::CounterDeviation(CounterDeviationEvent {
                machine_id: env.source.machine_id,
                counter_name: d.counter_name,
                expected: d.expected,
                actual: d.actual,
                tolerance: d.tolerance,
                severity: d.severity,
                message: d.message,
                ts: env.ts,
            }),
        }
    }
}

impl UpstreamInterface for DefaultUpstream {
    fn decode_events(&self, msg: &MqttMessage) -> Vec<GatewayEvent> {
        let text = match std::str::from_utf8(&msg.payload) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        // 1) Strict structured decoding
        if let Some(evts) = self.decode_structured(msg, text) {
            return evts;
        }

        // 2) Telemetry fallback parsing (your curly tuple / raw values / json telemetry)
        let samples = parser::parse_payload(text, &msg.topic);
        if samples.is_empty() {
            return vec![];
        }

        samples
            .into_iter()
            .map(|s| {
                let (num, unit) = parser::parse_value_and_unit(&s.data_raw);
                let value = match num {
                    Some(v) => Value::Number { v, unit },
                    None if !s.data_raw.trim().is_empty() => Value::Text(s.data_raw.clone()),
                    None => Value::Unknown,
                };

                GatewayEvent::Telemetry(TelemetryEvent {
                    topic: s.logical_topic,
                    data_raw: s.data_raw,
                    value,
                    ts: s.ts,
                    source_mqtt_topic: msg.topic.clone(),
                })
            })
            .collect()
    }

    fn encode_command(&self, cmd: &Command) -> Result<MqttPublish> {
        cmd.to_mqtt(&self.cfg.command_prefix)
    }
}
