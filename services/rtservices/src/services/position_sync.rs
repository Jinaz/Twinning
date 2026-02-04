//! Position Synchronization RT Service
//!
//! Synchronizes position data between Gateway and Database with RT guarantees.
//! Uses non-blocking operations (try_push, try_set_position) to maintain RT properties.

use crate::context::RtContext;
use crate::rt_checker::RtResult;
use crate::traits::{RtConfig, RtService};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Atomic position representation for lock-free RT access
#[derive(Debug)]
pub struct AtomicPosition {
    // Store as fixed-point integers (multiply by 1000 for 3 decimal places)
    x: AtomicU64,
    y: AtomicU64,
    z: AtomicU64,
}

impl AtomicPosition {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x: AtomicU64::new(Self::to_fixed(x)),
            y: AtomicU64::new(Self::to_fixed(y)),
            z: AtomicU64::new(Self::to_fixed(z)),
        }
    }

    fn to_fixed(val: f64) -> u64 {
        ((val + 1_000_000.0) * 1000.0) as u64 // Offset to handle negatives
    }

    fn from_fixed(val: u64) -> f64 {
        (val as f64 / 1000.0) - 1_000_000.0
    }

    pub fn load(&self) -> (f64, f64, f64) {
        (
            Self::from_fixed(self.x.load(Ordering::Acquire)),
            Self::from_fixed(self.y.load(Ordering::Acquire)),
            Self::from_fixed(self.z.load(Ordering::Acquire)),
        )
    }

    pub fn store(&self, x: f64, y: f64, z: f64) {
        self.x.store(Self::to_fixed(x), Ordering::Release);
        self.y.store(Self::to_fixed(y), Ordering::Release);
        self.z.store(Self::to_fixed(z), Ordering::Release);
    }
}

impl Default for AtomicPosition {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

/// Position Sync Service - Synchronizes position between gateway and database
///
/// This is an RT service that:
/// - Reads position from gateway (atomic, non-blocking)
/// - Writes to database using fire-and-forget pattern (non-blocking)
/// - Tracks sync statistics
pub struct PositionSyncService {
    name: String,
    config: RtConfig,
    /// Source position (from gateway)
    source_position: Arc<AtomicPosition>,
    /// Last synced position (cached)
    last_synced: (f64, f64, f64),
    /// Sync threshold - only sync if position changed by more than this
    threshold: f64,
    /// Statistics
    sync_count: u64,
    skip_count: u64,
}

impl PositionSyncService {
    pub fn new(
        name: &str,
        source_position: Arc<AtomicPosition>,
        period_us: u64,
    ) -> Self {
        Self {
            name: name.to_string(),
            config: RtConfig::new(period_us, period_us, period_us / 2),
            source_position,
            last_synced: (0.0, 0.0, 0.0),
            threshold: 0.001, // 1mm threshold
            sync_count: 0,
            skip_count: 0,
        }
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn with_config(mut self, config: RtConfig) -> Self {
        self.config = config;
        self
    }

    /// Check if position has changed significantly
    fn position_changed(&self, new_pos: (f64, f64, f64)) -> bool {
        let (lx, ly, lz) = self.last_synced;
        let (nx, ny, nz) = new_pos;

        let dx = (nx - lx).abs();
        let dy = (ny - ly).abs();
        let dz = (nz - lz).abs();

        dx > self.threshold || dy > self.threshold || dz > self.threshold
    }

    /// Get sync statistics
    pub fn stats(&self) -> (u64, u64) {
        (self.sync_count, self.skip_count)
    }
}

impl RtService for PositionSyncService {
    fn name(&self) -> &str {
        &self.name
    }

    fn config(&self) -> &RtConfig {
        &self.config
    }

    fn tick(&mut self, _ctx: &RtContext<'_>) -> RtResult<()> {
        // 1. Read position atomically (non-blocking)
        let new_pos = self.source_position.load();

        // 2. Check if sync needed (avoid unnecessary DB writes)
        if !self.position_changed(new_pos) {
            self.skip_count += 1;
            return Ok(());
        }

        // 3. Fire-and-forget write to database would happen here
        // In a real implementation, this would call:
        // db.try_set_position(entity_id, Position { x, y, z })
        //
        // For now, we just update our cached state
        self.last_synced = new_pos;
        self.sync_count += 1;

        log::trace!(
            "PositionSync: synced ({:.3}, {:.3}, {:.3}), total={}",
            new_pos.0, new_pos.1, new_pos.2, self.sync_count
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_position() {
        let pos = AtomicPosition::new(1.5, -2.5, 3.5);
        let (x, y, z) = pos.load();
        assert!((x - 1.5).abs() < 0.001);
        assert!((y - (-2.5)).abs() < 0.001);
        assert!((z - 3.5).abs() < 0.001);

        pos.store(10.0, 20.0, 30.0);
        let (x, y, z) = pos.load();
        assert!((x - 10.0).abs() < 0.001);
        assert!((y - 20.0).abs() < 0.001);
        assert!((z - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_position_sync_service() {
        use crate::properties::Properties;
        use crate::engine_event::EngineEvent;

        let source = Arc::new(AtomicPosition::new(0.0, 0.0, 0.0));
        let mut service = PositionSyncService::new("test_sync", source.clone(), 10_000);

        let props = Properties::new();
        let (event_tx, _event_rx) = crossbeam_channel::unbounded::<EngineEvent>();
        let ctx = RtContext {
            props: &props,
            event_tx: &event_tx,
        };

        // First tick should sync (initial position)
        service.tick(&ctx).unwrap();
        let (_syncs, skips) = service.stats();
        assert_eq!(skips, 1); // No change from initial

        // Change position significantly
        source.store(1.0, 1.0, 1.0);
        service.tick(&ctx).unwrap();
        let (syncs, _skips) = service.stats();
        assert_eq!(syncs, 1);

        // Small change - should skip
        source.store(1.0001, 1.0001, 1.0001);
        service.tick(&ctx).unwrap();
        let (_syncs, skips) = service.stats();
        assert_eq!(skips, 2);
    }
}
