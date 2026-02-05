use anyhow::Result;
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;

use crate::downstream::{Configuration, Method, Property};
use crate::downstream::configuration::Id;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::downstream::IDownstream;
use crate::upstream::IUpstream;


#[derive(Clone)]
pub struct GatewayConfig {
    /// Downstream holds configuration + method/property models.
    /// Wrap in Arc<RwLock<..>> so it can be shared + mutated safely.
    pub downstream: Arc<RwLock<IDownstream>>,

    /// Upstream typically holds event bus + command/property registries.
    /// Keep it shared too.
    pub upstream: Arc<IUpstream>,
}

#[async_trait]
pub trait GatewayApi: Send + Sync {
    async fn configure_config(&self, conf: Configuration) -> Result<()>;
    async fn configure_property(&self, id: Id, prop: Property) -> Result<()>;
    async fn configure_method(&self, id: Id, method: Method) -> Result<()>;

    async fn execute_method(&self, name: &Id) -> Result<()>;
    async fn run(&self) -> Result<()>;
    fn shutdown(&self);
}


/// Gateway = core state + lifecycle control (cancellation)
pub struct Gateway {
    pub config: GatewayConfig,
    cancel: CancellationToken,
}

impl Gateway {
    pub fn new(downstream: IDownstream, upstream: IUpstream) -> Self {
        Self {
            config: GatewayConfig {
                downstream: Arc::new(RwLock::new(downstream)),
                upstream: Arc::new(upstream),
            },
            cancel: CancellationToken::new(),
        }
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn shutdown(&self) {
        self.cancel.cancel();
    }

    /// Placeholder “run loop” – later you wire MQTT/OPC-UA tasks in here.
    pub async fn run(&self) -> Result<()> {
        // Example: wait until cancelled
        self.cancel.cancelled().await;
        Ok(())
    }
}

#[async_trait]
impl crate::GatewayApi for Gateway {
    async fn configure_config(&self, conf: Configuration) -> Result<()> {
        let mut ds = self.config.downstream.write().await;
        ds.set_configuration(conf);
        Ok(())
    }

    async fn configure_property(&self, id: Id, prop: Property) -> Result<()> {
        let mut ds = self.config.downstream.write().await;
        ds.upsert_property(id, prop);
        Ok(())
    }

    async fn configure_method(&self, id: Id, method: Method) -> Result<()> {
        let mut ds = self.config.downstream.write().await;
        ds.upsert_method(id, method);
        Ok(())
    }

    async fn execute_method(&self, name: &Id) -> Result<()> {
        // default: just checks presence; real transports override this in their own type
        let ds = self.config.downstream.read().await;
        anyhow::ensure!(ds.method(name).is_some(), "unknown method id: {name}");
        Ok(())
    }

    async fn run(&self) -> Result<()> {
        self.run().await
    }

    fn shutdown(&self) {
        self.shutdown()
    }
}