//! ServiceA - Sample RT Service
//!
//! Demonstrates a minimal RT service that:
//! - Increments a counter each tick
//! - Reads the current speed from shared properties
//! - Sends a SetSpeed event every 50 ticks

use crate::context::RtContext;
use crate::engine_event::EngineEvent;
use crate::rt_checker::RtResult;
use crate::traits::{RtConfig, RtService};

/// Sample RT service for demonstration and testing.
pub struct ServiceA {
    config: RtConfig,
    counter: u64,
}

impl ServiceA {
    pub fn new() -> Self {
        Self {
            config: RtConfig::default(),
            counter: 0,
        }
    }

    pub fn with_config(mut self, config: RtConfig) -> Self {
        self.config = config;
        self
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }
}

impl Default for ServiceA {
    fn default() -> Self {
        Self::new()
    }
}

impl RtService for ServiceA {
    fn name(&self) -> &str {
        "ServiceA"
    }

    fn config(&self) -> &RtConfig {
        &self.config
    }

    fn tick(&mut self, ctx: &RtContext<'_>) -> RtResult<()> {
        self.counter += 1;
        let s = ctx.props.get_speed();

        if self.counter % 50 == 0 {
            let _ = ctx.event_tx.try_send(EngineEvent::SetSpeed(s + 0.5));
            log::trace!("ServiceA: tick={}, speed={:.2}, sent SetSpeed", self.counter, s);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::Properties;

    #[test]
    fn test_service_a_ticks() {
        let mut svc = ServiceA::new();
        let props = Properties::new();
        props.set_speed(10.0);
        let (tx, rx) = crossbeam_channel::unbounded();
        let ctx = RtContext {
            props: &props,
            event_tx: &tx,
        };

        for _ in 0..50 {
            svc.tick(&ctx).unwrap();
        }

        assert_eq!(svc.counter(), 50);
        // Should have sent exactly one event at tick 50
        assert_eq!(rx.len(), 1);
        match rx.try_recv().unwrap() {
            EngineEvent::SetSpeed(v) => assert!((v - 10.5).abs() < f64::EPSILON),
            _ => panic!("expected SetSpeed"),
        }
    }
}
