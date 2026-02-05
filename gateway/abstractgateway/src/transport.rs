use anyhow::Result;
use async_trait::async_trait;

use crate::abstractgateway::Gateway;

#[async_trait]
pub trait GatewayTransport: Send + Sync {
    /// Called once to initialize network connections.
    async fn connect(&mut self) -> Result<()>;

    /// Runs transport tasks (subscribe loops, publish loops).
    /// Should return when `gateway.shutdown()` is triggered.
    async fn run(&mut self, gateway: &Gateway) -> Result<()>;
}
