//! Real-Time Service Traits
//!
//! Defines the contract for RT services with bounded execution time.

use crate::context::RtContext;
use crate::rt_checker::RtResult;
use std::time::Duration;

/// Configuration for an RT service's timing constraints
#[derive(Debug, Clone)]
pub struct RtConfig {
    /// Period at which the service should be called
    pub period: Duration,
    /// Maximum allowed latency (deadline)
    pub deadline: Duration,
    /// Worst-Case Execution Time budget
    pub wcet: Duration,
    /// CPU core to pin to (optional)
    pub cpu_core: Option<usize>,
    /// FIFO priority (1-99, optional)
    pub fifo_priority: Option<i32>,
}

impl RtConfig {
    pub fn new(period_us: u64, deadline_us: u64, wcet_us: u64) -> Self {
        Self {
            period: Duration::from_micros(period_us),
            deadline: Duration::from_micros(deadline_us),
            wcet: Duration::from_micros(wcet_us),
            cpu_core: None,
            fifo_priority: None,
        }
    }

    pub fn with_cpu_core(mut self, core: usize) -> Self {
        self.cpu_core = Some(core);
        self
    }

    pub fn with_fifo_priority(mut self, priority: i32) -> Self {
        self.fifo_priority = Some(priority);
        self
    }
}

impl Default for RtConfig {
    fn default() -> Self {
        Self {
            period: Duration::from_millis(10),      // 10ms default period
            deadline: Duration::from_millis(10),    // deadline = period
            wcet: Duration::from_millis(5),         // 50% utilization
            cpu_core: None,
            fifo_priority: None,
        }
    }
}

/// Real-Time Service Trait
///
/// Services implementing this trait have bounded execution time guarantees.
/// The `tick` method must be non-blocking and complete within the WCET budget.
pub trait RtService: Send + Sync {
    /// Unique name of the service
    fn name(&self) -> &str;

    /// Get RT timing configuration
    fn config(&self) -> &RtConfig;

    /// Period in microseconds
    fn period_us(&self) -> u64 {
        self.config().period.as_micros() as u64
    }

    /// Deadline in microseconds
    fn deadline_us(&self) -> u64 {
        self.config().deadline.as_micros() as u64
    }

    /// Worst-Case Execution Time in microseconds
    fn wcet_us(&self) -> u64 {
        self.config().wcet.as_micros() as u64
    }

    /// Initialize the service (called once before tick loop starts)
    fn init(&mut self) -> RtResult<()> {
        Ok(())
    }

    /// RT-safe tick - MUST be non-blocking and complete within WCET
    ///
    /// This method is called at the configured period. It must:
    /// - Not perform any blocking I/O
    /// - Not allocate memory (if possible)
    /// - Not wait on locks (use try_lock patterns)
    /// - Complete within the WCET budget
    fn tick(&mut self, ctx: &RtContext<'_>) -> RtResult<()>;

    /// Shutdown the service (called once when stopping)
    fn shutdown(&mut self) -> RtResult<()> {
        Ok(())
    }
}

/// Extension trait for boxed RT services
pub trait RtServiceExt {
    fn period(&self) -> Duration;
    fn deadline(&self) -> Duration;
    fn wcet(&self) -> Duration;
}

impl<T: RtService + ?Sized> RtServiceExt for T {
    fn period(&self) -> Duration {
        self.config().period
    }

    fn deadline(&self) -> Duration {
        self.config().deadline
    }

    fn wcet(&self) -> Duration {
        self.config().wcet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRtService {
        name: String,
        config: RtConfig,
        tick_count: u32,
    }

    impl TestRtService {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                config: RtConfig::default(),
                tick_count: 0,
            }
        }
    }

    impl RtService for TestRtService {
        fn name(&self) -> &str {
            &self.name
        }

        fn config(&self) -> &RtConfig {
            &self.config
        }

        fn tick(&mut self, _ctx: &RtContext<'_>) -> RtResult<()> {
            self.tick_count += 1;
            Ok(())
        }
    }

    #[test]
    fn test_rt_service_trait() {
        use crate::properties::Properties;
        use crate::engine_event::EngineEvent;

        let mut service = TestRtService::new("test");
        assert_eq!(service.name(), "test");
        assert_eq!(service.period_us(), 10_000); // 10ms in us

        let props = Properties::new();
        let (event_tx, _event_rx) = crossbeam_channel::unbounded::<EngineEvent>();
        let ctx = RtContext {
            props: &props,
            event_tx: &event_tx,
        };

        service.tick(&ctx).unwrap();
        assert_eq!(service.tick_count, 1);
    }
}
