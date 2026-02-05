use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use crate::downstream::configuration::Id as DSCFGID;
use crate::downstream::method::Id as DSID;
use crate::upstream::event::Id as USID;
use tokio::runtime::Id;
use crate::upstream::event::EventEnvelope;
use crate::downstream::property::Property as DSProperty;
use crate::upstream::property::Property as USProperty;
use crate::upstream::command::Command;
/// Simple async registry for Properties / Commands (CRUD-ish).
#[derive(Debug, Default)]
pub struct Registry<T> {
    inner: RwLock<HashMap<Id, T>>,
}

impl<T: Clone> Registry<T> {
    pub fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }

    pub async fn upsert(&self, id: Id, value: T) {
        self.inner.write().await.insert(id, value);
    }

    pub async fn get(&self, id: &Id) -> Option<T> {
        self.inner.read().await.get(id).cloned()
    }

    pub async fn remove(&self, id: &Id) -> Option<T> {
        self.inner.write().await.remove(id)
    }

    pub async fn list(&self) -> HashMap<Id, T> {
        self.inner.read().await.clone()
    }
}

/// Reusable event mechanism (fan-out).
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<EventEnvelope>,
}

impl EventBus {
    pub fn new(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel(buffer);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.tx.subscribe()
    }

    pub fn publish(&self, evt: EventEnvelope) -> Result<usize, broadcast::error::SendError<EventEnvelope>> {
        self.tx.send(evt)
    }
}

/// IUpstream has: properties, commands, events (as you requested).
#[derive(Clone)]
pub struct IUpstream {
    pub properties: Arc<Registry<USProperty>>,
    pub commands: Arc<Registry<Command>>,
    pub events: EventBus,
}

impl IUpstream {
    pub fn new(event_buffer: usize) -> Self {
        Self {
            properties: Arc::new(Registry::new()),
            commands: Arc::new(Registry::new()),
            events: EventBus::new(event_buffer),
        }
    }
}