use chrono::Utc;

use super::eventqueue::{EventEnvelope, EventKind, EventQueue, QueueError};

pub struct WorkflowEngine {
    queue: EventQueue,
    /// Max events per tick (keeps runtime predictable).
    max_per_tick: usize,
}

impl WorkflowEngine {
    pub fn new(queue: EventQueue) -> Self {
        Self {
            queue,
            max_per_tick: 32,
        }
    }

    pub fn with_budget(mut self, max_per_tick: usize) -> Self {
        self.max_per_tick = max_per_tick.max(1);
        self
    }

    /// Non-blocking: process up to max_per_tick if available.
    pub async fn tick(&self) {
        for _ in 0..self.max_per_tick {
            let env = match self.queue.try_pop() {
                Ok(ev) => ev,
                Err(QueueError::Empty) => return,
                Err(QueueError::Busy) => return, // keep RT-friendly; try again next loop
                Err(QueueError::Full) => unreachable!("pop cannot be Full"),
            };

            self.handle_event(env).await;
        }
    }

    async fn handle_event(&self, env: EventEnvelope) {
        // --- your event handling logic goes here ---
        match &env.event.kind {
            EventKind::PositionChanged => {
                // placeholder: trigger some workflow action
            }
            EventKind::SyncCompleted => {}
            EventKind::Custom(_) => {}
        }

        // "handled" logging + latency
        let handled_at = Utc::now();
        let latency = handled_at - env.arrived_at;

        log::info!(
            "event handled: id={} kind={:?} handled_at={} latency_ms={}",
            env.event.id,
            env.event.kind,
            handled_at,
            latency.num_milliseconds()
        );
    }
}
