//! Engine events for communication between services and the engine loop.

#[derive(Debug, Clone)]
pub enum EngineEvent {
    SetSpeed(f64),
    Custom {
        topic: String,
        payload: serde_json::Value,
    },
}
