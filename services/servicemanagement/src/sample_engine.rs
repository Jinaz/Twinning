//! Sample Engine
//!
//! Minimal Digital Twin engine that demonstrates the full service pipeline:
//! - Periodic RT tick loop with budget checking
//! - Event processing (SetSpeed, Custom)
//! - Shared atomic properties

use std::sync::Arc;
use std::time::{Duration, Instant};

use rtservices::{EngineEvent, Properties};

use crate::manager::ServiceManager;

/// Minimal DT engine for demonstration and testing.
pub struct SampleEngine {
    sm: ServiceManager,
}

impl SampleEngine {
    pub fn new(sm: ServiceManager) -> Self {
        Self { sm }
    }

    /// Access the service manager
    pub fn servicemanager(&self) -> &ServiceManager {
        &self.sm
    }

    /// Mutable access to the service manager
    pub fn servicemanager_mut(&mut self) -> &mut ServiceManager {
        &mut self.sm
    }

    /// Access shared properties
    pub fn props(&self) -> Arc<Properties> {
        self.sm.props()
    }

    /// Run the engine for a given duration at the specified period.
    /// Calls tick_rt() and process_events() each cycle.
    pub fn run_for(&mut self, run_time: Duration, period: Duration) {
        let start = Instant::now();
        let mut next = Instant::now() + period;

        while start.elapsed() < run_time {
            // RT tick
            self.sm.tick_rt();

            // Process engine events
            self.process_events();

            // Increment speed slightly (simulates engine work)
            let s = self.sm.props().get_speed();
            self.sm.props().set_speed(s + 0.01);

            // Sleep until next tick
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
            next += period;
        }
    }

    /// Drain and process all pending events from the event channel.
    pub fn process_events(&mut self) {
        let rx = self.sm.event_rx();
        while let Ok(event) = rx.try_recv() {
            match event {
                EngineEvent::SetSpeed(v) => {
                    self.sm.props().set_speed(v);
                    log::debug!("Engine: SetSpeed -> {:.2}", v);
                }
                EngineEvent::Custom { topic, payload } => {
                    log::debug!("Engine: Custom event topic={} payload={}", topic, payload);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::RtRegistration;
    use rtservices::services::ServiceA;

    #[test]
    fn test_sample_engine_run() {
        let mut sm = ServiceManager::new();
        sm.register_rt(
            Box::new(ServiceA::new()),
            RtRegistration {
                budget: Duration::from_millis(5),
            },
        );

        let mut engine = SampleEngine::new(sm);
        engine.props().set_speed(1.0);

        // Run for 100ms at 10ms period = ~10 ticks
        engine.run_for(Duration::from_millis(100), Duration::from_millis(10));

        // Speed should have increased
        assert!(engine.props().get_speed() > 1.0);

        // Stats should show calls
        let stats = engine.servicemanager().rt_stats_snapshot();
        assert!(!stats.is_empty());
        assert!(stats[0].1.calls > 0);
    }
}
