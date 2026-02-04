//! RT timing guard and per-service statistics.

use std::time::{Duration, Instant};

/// Timer guard for measuring RT service execution time.
pub struct RtGuard {
    start: Instant,
}

impl RtGuard {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Default for RtGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-service RT execution statistics.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RtServiceStats {
    pub calls: u64,
    pub overruns: u32,
    pub last_elapsed: Duration,
    pub max_elapsed: Duration,
    pub disabled: bool,
}

/// Configuration for RT overrun checking.
#[derive(Debug, Clone)]
pub struct RtCheckConfig {
    pub disable_after_overruns: u32,
}

impl Default for RtCheckConfig {
    fn default() -> Self {
        Self {
            disable_after_overruns: 3,
        }
    }
}

/// Update stats after a tick. Returns true if the service is now disabled.
pub fn update_stats(
    stats: &mut RtServiceStats,
    elapsed: Duration,
    budget: Duration,
    cfg: &RtCheckConfig,
) -> bool {
    stats.calls += 1;
    stats.last_elapsed = elapsed;
    if elapsed > stats.max_elapsed {
        stats.max_elapsed = elapsed;
    }
    if elapsed > budget {
        stats.overruns += 1;

        log::warn!(
            "RT overrun: elapsed={:?} budget={:?} (overrun #{}/{})",
            elapsed,
            budget,
            stats.overruns,
            cfg.disable_after_overruns
        );
        if stats.overruns >= cfg.disable_after_overruns {
            stats.disabled = true;
        }
    }
    stats.disabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_stats_no_overrun() {
        let mut stats = RtServiceStats::default();
        let cfg = RtCheckConfig::default();
        let disabled = update_stats(
            &mut stats,
            Duration::from_micros(100),
            Duration::from_micros(500),
            &cfg,
        );
        assert!(!disabled);
        assert_eq!(stats.calls, 1);
        assert_eq!(stats.overruns, 0);
    }

    #[test]
    fn test_update_stats_overrun_disables() {
        let mut stats = RtServiceStats::default();
        let cfg = RtCheckConfig {
            disable_after_overruns: 3,
        };
        let budget = Duration::from_micros(100);
        let over = Duration::from_micros(200);

        update_stats(&mut stats, over, budget, &cfg);
        update_stats(&mut stats, over, budget, &cfg);
        let disabled = update_stats(&mut stats, over, budget, &cfg);
        assert!(disabled);
        assert!(stats.disabled);
        assert_eq!(stats.overruns, 3);
    }
}
