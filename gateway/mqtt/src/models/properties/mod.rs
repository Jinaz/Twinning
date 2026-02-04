use crate::models::events::TelemetryEvent;

pub mod position;
pub mod speed;

pub use position::{PositionProperty, PositionState};
pub use speed::{SpeedProperty, SpeedState};

/// Holds all tracked properties (extend as you add more).
#[derive(Debug, Default)]
pub struct PropertyModel {
    pub position: PositionProperty,
    pub speed: SpeedProperty,
}

impl PropertyModel {
    /// Called by the gateway whenever telemetry comes in.
    pub fn apply_telemetry(&mut self, t: &TelemetryEvent) {
        self.position.update_from_telemetry(t);
        self.speed.update_from_telemetry(t);
        // add more properties here later
    }
}
