
use anyhow::{anyhow, Context, Result};
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use abstractdatabase::client::{ManagedConnection, ConnectionFactory, ConnectorConfig, Connector};
//
// -------- Example: plug in a fake transport to show usage --------
// Replace this with your DB-specific connector (TCP dial + auth + protocol handshake).
//

#[derive(Debug)]
struct DummyConn {
    healthy: bool,
}

#[async_trait::async_trait]
impl ManagedConnection for DummyConn {
    async fn is_healthy(&mut self) -> bool {
        self.healthy
    }

    async fn reset(&mut self) -> Result<()> {
        // pretend to rollback, clear state
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.healthy = false;
        Ok(())
    }
}

#[derive(Debug)]
struct DummyFactory;

#[async_trait::async_trait]
impl ConnectionFactory for DummyFactory {
    type Conn = DummyConn;

    async fn connect(&self) -> Result<Self::Conn> {
        Ok(DummyConn { healthy: true })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = ConnectorConfig {
        max_size: 4,
        min_size: 0,
        checkout_timeout: Duration::from_secs(2),
        max_idle: Some(Duration::from_secs(60)),
        prewarm_interval: Duration::from_secs(10),
    };

    let connector = Connector::new(DummyFactory, cfg);

    // Optional: connector.start_prewarm();

    // Checkout a session
    let mut s = connector.session().await?;
    {
        let conn = s.conn_mut()?;
        // use conn for protocol calls
        let _ = conn.is_healthy().await;
    }

    // Drop returns it to pool (async via tokio::spawn)
    drop(s);

    Ok(())
}