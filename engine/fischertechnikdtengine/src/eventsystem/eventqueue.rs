use std::{fmt, sync::Arc};
use tokio::sync::{Mutex, Notify};

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum EventSource {
    Gateway,
    Database,
    ModelManager,
    Service,
    Other(String),
}

#[derive(Debug, Clone)]
pub enum EventKind {
    PositionChanged,
    SyncCompleted,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: Uuid,
    pub kind: EventKind,
    pub source: EventSource,
    pub created_at: DateTime<Utc>,
}

impl Event {
    pub fn new(kind: EventKind, source: EventSource) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            source,
            created_at: Utc::now(),
        }
    }
}

/// Returned by the queue; contains the arrival timestamp for "handled latency" logging later.
#[derive(Debug, Clone)]
pub struct EventEnvelope {
    pub event: Event,
    pub arrived_at: DateTime<Utc>,
}

#[derive(thiserror::Error, Debug)]
pub enum QueueError {
    #[error("queue is full")]
    Full,
    #[error("queue is empty")]
    Empty,
    #[error("queue mutex busy")]
    Busy,
}

struct RingState {
    buf: Vec<Option<EventEnvelope>>,
    head: usize,
    tail: usize,
    len: usize,
}

impl RingState {
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![None; capacity],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.buf.len()
    }

    fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, item: EventEnvelope) -> Result<(), QueueError> {
        if self.is_full() {
            return Err(QueueError::Full);
        }
        self.buf[self.tail] = Some(item);
        self.tail = (self.tail + 1) % self.capacity();
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<EventEnvelope, QueueError> {
        if self.is_empty() {
            return Err(QueueError::Empty);
        }
        let item = self.buf[self.head].take().expect("ringbuffer slot empty");
        self.head = (self.head + 1) % self.capacity();
        self.len -= 1;
        Ok(item)
    }
}

#[derive(Clone)]
pub struct EventQueue {
    inner: Arc<Mutex<RingState>>,
    not_empty: Arc<Notify>,
    not_full: Arc<Notify>,
}

impl fmt::Debug for EventQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventQueue").finish()
    }
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be > 0");
        Self {
            inner: Arc::new(Mutex::new(RingState::new(capacity))),
            not_empty: Arc::new(Notify::new()),
            not_full: Arc::new(Notify::new()),
        }
    }

    /// Non-blocking push (RT-friendly). Uses try_lock; if busy/full -> error immediately.
    pub fn try_push(&self, event: Event) -> Result<(), QueueError> {
        let mut guard = self.inner.try_lock().map_err(|_| QueueError::Busy)?;
        if guard.is_full() {
            return Err(QueueError::Full);
        }

        let envelope = EventEnvelope {
            arrived_at: Utc::now(),
            event: event.clone(),
        };

        guard.push(envelope)?;
        self.not_empty.notify_one();

        log::info!(
            "event arrived: id={} kind={:?} source={:?} at={}",
            event.id,
            event.kind,
            event.source,
            Utc::now()
        );

        Ok(())
    }

    /// Awaiting push (backpressure): waits until there is capacity.
    pub async fn push(&self, event: Event) -> Result<(), QueueError> {
        loop {
            {
                let mut guard = self.inner.lock().await;
                if !guard.is_full() {
                    let envelope = EventEnvelope {
                        arrived_at: Utc::now(),
                        event: event.clone(),
                    };

                    guard.push(envelope)?;
                    self.not_empty.notify_one();

                    log::info!(
                        "event arrived: id={} kind={:?} source={:?} at={}",
                        event.id,
                        event.kind,
                        event.source,
                        Utc::now()
                    );

                    return Ok(());
                }
            }
            self.not_full.notified().await;
        }
    }

    /// Non-blocking pop. Returns Empty if none available.
    pub fn try_pop(&self) -> Result<EventEnvelope, QueueError> {
        let mut guard = self.inner.try_lock().map_err(|_| QueueError::Busy)?;
        let ev = guard.pop()?;
        self.not_full.notify_one();
        Ok(ev)
    }

    /// Awaiting pop: waits until there is at least one event.
    pub async fn pop(&self) -> Result<EventEnvelope, QueueError> {
        loop {
            {
                let mut guard = self.inner.lock().await;
                if !guard.is_empty() {
                    let ev = guard.pop()?;
                    self.not_full.notify_one();
                    return Ok(ev);
                }
            }
            self.not_empty.notified().await;
        }
    }
}
