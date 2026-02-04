//! Atomic in-memory properties for lock-free RT access.

use std::sync::atomic::{AtomicU64, Ordering};

/// Shared properties accessible from both RT and Non-RT contexts.
/// Uses AtomicU64 with f64 bit-casting for lock-free reads/writes.
pub struct Properties {
    speed: AtomicU64,
}

impl Properties {
    pub fn new() -> Self {
        Self {
            speed: AtomicU64::new(0f64.to_bits()),
        }
    }

    pub fn get_speed(&self) -> f64 {
        f64::from_bits(self.speed.load(Ordering::Acquire))
    }

    pub fn set_speed(&self, v: f64) {
        self.speed.store(v.to_bits(), Ordering::Release);
    }
}

impl Default for Properties {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_atomic() {
        let props = Properties::new();
        assert_eq!(props.get_speed(), 0.0);

        props.set_speed(42.5);
        assert!((props.get_speed() - 42.5).abs() < f64::EPSILON);

        props.set_speed(-1.0);
        assert!((props.get_speed() - (-1.0)).abs() < f64::EPSILON);
    }
}
