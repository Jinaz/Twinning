//! Real-Time Checker - OS-Level RT Verification
//!
//! Provides functionality for:
//! - CPU Pinning (sched_setaffinity)
//! - SCHED_FIFO Scheduler Priority
//! - Memory Locking (mlockall)
//! - Jitter Monitoring

use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RtError {
    #[error("CPU pinning failed: {0}")]
    CpuPinning(String),
    #[error("SCHED_FIFO setup failed: {0}")]
    SchedFifo(String),
    #[error("Memory locking failed: {0}")]
    MemoryLock(String),
    #[error("RT check failed: {0}")]
    CheckFailed(String),
    #[error("Platform not supported for RT features")]
    PlatformNotSupported,
}

pub type RtResult<T> = Result<T, RtError>;

/// Jitter statistics collected during RT monitoring
#[derive(Debug, Clone, Default)]
pub struct JitterStats {
    pub min_us: u64,
    pub max_us: u64,
    pub avg_us: f64,
    pub samples: u64,
    pub histogram: [u64; 10], // Buckets: 0-10us, 10-20us, ..., 90-100us+
}

impl JitterStats {
    pub fn new() -> Self {
        Self {
            min_us: u64::MAX,
            max_us: 0,
            avg_us: 0.0,
            samples: 0,
            histogram: [0; 10],
        }
    }

    pub fn record(&mut self, jitter_us: u64) {
        self.min_us = self.min_us.min(jitter_us);
        self.max_us = self.max_us.max(jitter_us);

        // Rolling average
        self.samples += 1;
        self.avg_us = self.avg_us + (jitter_us as f64 - self.avg_us) / self.samples as f64;

        // Histogram bucket (10us buckets)
        let bucket = (jitter_us / 10).min(9) as usize;
        self.histogram[bucket] += 1;
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Snapshot of RT monitor statistics.
#[derive(Debug, Clone, Default)]
pub struct RtStats {
    pub samples: u64,
    pub misses: u64,
    pub max_late_ns: u64,
}

/// Real-Time Checker for OS-level RT verification and monitoring
pub struct RtChecker {
    /// CPU core this checker is pinned to (if any)
    pinned_core: Option<usize>,
    /// FIFO priority (if set)
    fifo_priority: Option<i32>,
    /// Whether memory is locked
    memory_locked: bool,
    /// Jitter statistics
    jitter_stats: JitterStats,
    /// Last tick time for jitter calculation
    last_tick: Option<Instant>,
    /// Expected period for jitter calculation
    expected_period: Option<Duration>,
    /// Monitor thread handle and atomics
    #[cfg(unix)]
    monitor: Option<MonitorHandle>,
}

impl RtChecker {
    pub fn new() -> Self {
        Self {
            pinned_core: None,
            fifo_priority: None,
            memory_locked: false,
            jitter_stats: JitterStats::new(),
            last_tick: None,
            expected_period: None,
            #[cfg(unix)]
            monitor: None,
        }
    }

    /// Set expected period for jitter monitoring
    pub fn with_period(mut self, period: Duration) -> Self {
        self.expected_period = Some(period);
        self
    }

    /// Pin current thread to a specific CPU core
    #[cfg(unix)]
    pub fn pin_to_cpu(&mut self, core_id: usize) -> RtResult<()> {
        use nix::sched::{sched_setaffinity, CpuSet};
        use nix::unistd::Pid;

        let mut cpu_set = CpuSet::new();
        cpu_set.set(core_id).map_err(|e| RtError::CpuPinning(e.to_string()))?;

        sched_setaffinity(Pid::from_raw(0), &cpu_set)
            .map_err(|e| RtError::CpuPinning(e.to_string()))?;

        self.pinned_core = Some(core_id);
        log::info!("RT: Pinned to CPU core {}", core_id);
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn pin_to_cpu(&mut self, _core_id: usize) -> RtResult<()> {
        log::warn!("RT: CPU pinning not supported on this platform");
        Err(RtError::PlatformNotSupported)
    }

    /// Set SCHED_FIFO real-time scheduler with given priority
    #[cfg(unix)]
    pub fn set_fifo_priority(&mut self, priority: i32) -> RtResult<()> {
        use nix::sched::{sched_setscheduler, Scheduler};
        use nix::sys::sched::SchedPolicy;
        use nix::unistd::Pid;

        // Priority must be 1-99 for SCHED_FIFO
        let prio = priority.clamp(1, 99);

        let param = libc::sched_param {
            sched_priority: prio,
        };

        let result = unsafe {
            libc::sched_setscheduler(0, libc::SCHED_FIFO, &param)
        };

        if result != 0 {
            let err = std::io::Error::last_os_error();
            return Err(RtError::SchedFifo(format!(
                "sched_setscheduler failed: {} (try running as root)",
                err
            )));
        }

        self.fifo_priority = Some(prio);
        log::info!("RT: Set SCHED_FIFO with priority {}", prio);
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn set_fifo_priority(&mut self, _priority: i32) -> RtResult<()> {
        log::warn!("RT: SCHED_FIFO not supported on this platform");
        Err(RtError::PlatformNotSupported)
    }

    /// Lock all current and future memory pages (prevents page faults)
    #[cfg(unix)]
    pub fn lock_memory(&mut self) -> RtResult<()> {
        use nix::sys::mman::{mlockall, MlockAllFlags};

        mlockall(MlockAllFlags::MCL_CURRENT | MlockAllFlags::MCL_FUTURE)
            .map_err(|e| RtError::MemoryLock(format!(
                "mlockall failed: {} (try running as root or increase RLIMIT_MEMLOCK)",
                e
            )))?;

        self.memory_locked = true;
        log::info!("RT: Memory locked with mlockall");
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn lock_memory(&mut self) -> RtResult<()> {
        log::warn!("RT: Memory locking not supported on this platform");
        Err(RtError::PlatformNotSupported)
    }

    /// Apply all RT settings at once (convenience method)
    pub fn apply_rt_settings(&mut self, core_id: usize, priority: i32) -> RtResult<()> {
        // Try each setting, log warnings for failures but continue
        if let Err(e) = self.pin_to_cpu(core_id) {
            log::warn!("RT: Could not pin CPU: {}", e);
        }

        if let Err(e) = self.set_fifo_priority(priority) {
            log::warn!("RT: Could not set FIFO priority: {}", e);
        }

        if let Err(e) = self.lock_memory() {
            log::warn!("RT: Could not lock memory: {}", e);
        }

        Ok(())
    }

    /// Record a tick and calculate jitter from expected period
    pub fn tick(&mut self) -> Option<u64> {
        let now = Instant::now();

        if let (Some(last), Some(expected)) = (self.last_tick, self.expected_period) {
            let actual = now.duration_since(last);
            let jitter_us = if actual > expected {
                (actual - expected).as_micros() as u64
            } else {
                (expected - actual).as_micros() as u64
            };

            self.jitter_stats.record(jitter_us);
            self.last_tick = Some(now);
            return Some(jitter_us);
        }

        self.last_tick = Some(now);
        None
    }

    /// Get current jitter statistics
    pub fn jitter_stats(&self) -> &JitterStats {
        &self.jitter_stats
    }

    /// Reset jitter statistics
    pub fn reset_jitter_stats(&mut self) {
        self.jitter_stats.reset();
        self.last_tick = None;
    }

    /// Check if RT features are active
    pub fn is_rt_active(&self) -> bool {
        self.pinned_core.is_some() || self.fifo_priority.is_some() || self.memory_locked
    }

    /// Start the monitor thread (unix only).
    #[cfg(unix)]
    pub fn start_monitor(&mut self, cpu: usize, fifo_prio: i32, period: Duration) {
        let (handle, stop, max_late_ns, misses, samples) =
            spawn_monitor_thread(cpu, fifo_prio, period);
        self.monitor = Some(MonitorHandle {
            handle: Some(handle),
            stop,
            max_late_ns,
            misses,
            samples,
        });
    }

    /// Get a snapshot of the monitor thread statistics.
    pub fn snapshot(&self) -> RtStats {
        #[cfg(unix)]
        if let Some(ref m) = self.monitor {
            return RtStats {
                samples: m.samples.load(std::sync::atomic::Ordering::Relaxed),
                misses: m.misses.load(std::sync::atomic::Ordering::Relaxed),
                max_late_ns: m.max_late_ns.load(std::sync::atomic::Ordering::Relaxed),
            };
        }
        RtStats::default()
    }

    /// Stop the monitor thread.
    pub fn stop(&mut self) {
        #[cfg(unix)]
        if let Some(mut m) = self.monitor.take() {
            m.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(h) = m.handle.take() {
                let _ = h.join();
            }
        }
    }

    /// Get RT status report
    pub fn status_report(&self) -> String {
        format!(
            "RT Status: CPU={}, FIFO={}, MemLock={}",
            self.pinned_core.map_or("none".to_string(), |c| c.to_string()),
            self.fifo_priority.map_or("none".to_string(), |p| p.to_string()),
            if self.memory_locked { "yes" } else { "no" }
        )
    }
}

/// Internal handle for the monitor thread and its atomic counters.
#[cfg(unix)]
struct MonitorHandle {
    handle: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    max_late_ns: std::sync::Arc<std::sync::atomic::AtomicU64>,
    misses: std::sync::Arc<std::sync::atomic::AtomicU64>,
    samples: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Spawn a dedicated RT monitor thread that uses clock_nanosleep (TIMER_ABSTIME)
/// to measure OS-level scheduling jitter. Returns a join handle and stop flag.
#[cfg(unix)]
pub fn spawn_monitor_thread(
    cpu: usize,
    fifo_prio: i32,
    period: std::time::Duration,
) -> (
    std::thread::JoinHandle<()>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Arc<std::sync::atomic::AtomicU64>, // max_late_ns
    std::sync::Arc<std::sync::atomic::AtomicU64>, // misses
    std::sync::Arc<std::sync::atomic::AtomicU64>, // samples
) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    let stop = Arc::new(AtomicBool::new(false));
    let max_late_ns = Arc::new(AtomicU64::new(0));
    let misses = Arc::new(AtomicU64::new(0));
    let samples = Arc::new(AtomicU64::new(0));

    let stop_clone = stop.clone();
    let max_late_clone = max_late_ns.clone();
    let misses_clone = misses.clone();
    let samples_clone = samples.clone();

    let period_ns = period.as_nanos() as i64;
    let deadline_ns = period_ns; // deadline = period

    let handle = std::thread::spawn(move || {
        // Apply RT settings on monitor thread
        unsafe {
            // CPU pinning
            let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_ZERO(&mut cpuset);
            libc::CPU_SET(cpu, &mut cpuset);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);

            // SCHED_FIFO
            let param = libc::sched_param {
                sched_priority: fifo_prio,
            };
            libc::sched_setscheduler(0, libc::SCHED_FIFO, &param);

            // mlockall
            libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
        }

        // Get initial time
        let mut next = unsafe {
            let mut ts: libc::timespec = std::mem::zeroed();
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
            ts
        };

        // Advance to first tick
        add_ns(&mut next, period_ns);

        while !stop_clone.load(Ordering::Relaxed) {
            // Absolute-time sleep
            unsafe {
                libc::clock_nanosleep(
                    libc::CLOCK_MONOTONIC,
                    libc::TIMER_ABSTIME,
                    &next,
                    std::ptr::null_mut(),
                );
            }

            // Measure lateness
            let now = unsafe {
                let mut ts: libc::timespec = std::mem::zeroed();
                libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
                ts
            };

            let late_ns = diff_ns(&now, &next);
            samples_clone.fetch_add(1, Ordering::Relaxed);

            if late_ns > deadline_ns {
                misses_clone.fetch_add(1, Ordering::Relaxed);
            }

            // Update max
            let late_u64 = late_ns.max(0) as u64;
            let mut current_max = max_late_clone.load(Ordering::Relaxed);
            while late_u64 > current_max {
                match max_late_clone.compare_exchange_weak(
                    current_max,
                    late_u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(v) => current_max = v,
                }
            }

            // Advance next by period
            add_ns(&mut next, period_ns);
        }
    });

    (handle, stop, max_late_ns, misses, samples)
}

#[cfg(unix)]
fn add_ns(ts: &mut libc::timespec, add: i64) {
    let mut nsec = ts.tv_nsec as i64 + add;
    ts.tv_sec += nsec / 1_000_000_000;
    nsec %= 1_000_000_000;
    if nsec < 0 {
        nsec += 1_000_000_000;
        ts.tv_sec -= 1;
    }
    ts.tv_nsec = nsec as _;
}

#[cfg(unix)]
fn diff_ns(a: &libc::timespec, b: &libc::timespec) -> i64 {
    (a.tv_sec - b.tv_sec) * 1_000_000_000 + (a.tv_nsec - b.tv_nsec) as i64
}

impl Default for RtChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jitter_stats() {
        let mut stats = JitterStats::new();
        stats.record(5);
        stats.record(15);
        stats.record(25);

        assert_eq!(stats.min_us, 5);
        assert_eq!(stats.max_us, 25);
        assert_eq!(stats.samples, 3);
        assert_eq!(stats.histogram[0], 1); // 0-10us bucket
        assert_eq!(stats.histogram[1], 1); // 10-20us bucket
        assert_eq!(stats.histogram[2], 1); // 20-30us bucket
    }

    #[test]
    fn test_rt_checker_creation() {
        let checker = RtChecker::new();
        assert!(!checker.is_rt_active());
    }
}
