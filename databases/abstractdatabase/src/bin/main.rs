
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

use std::time::Duration;
use abstractdatabase::protocol::{
    Codec, Protocol, ProtocolConnection, ProtocolFactory, Transport, TransportFactory,
};

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


#[derive(Debug)]
struct DummyTransport;

#[async_trait::async_trait]
impl Transport for DummyTransport {
    type Inbound = ();
    type Outbound = ();

    async fn send(&mut self, _msg: Self::Outbound) -> Result<()> { Ok(()) }
    async fn recv(&mut self) -> Result<Self::Inbound> { Ok(()) }
    async fn close(&mut self) -> Result<()> { Ok(()) }
}

#[derive(Debug)]
struct DummyTransportFactory;

#[async_trait::async_trait]
impl TransportFactory for DummyTransportFactory {
    type T = DummyTransport;

    async fn connect(&self) -> Result<Self::T> {
        Ok(DummyTransport)
    }
}

#[derive(Debug, Clone)]
struct PassthroughCodec;

#[async_trait::async_trait]
impl Codec for PassthroughCodec {
    type In = ();
    type Out = ();

    type FrameIn = ();
    type FrameOut = ();

    async fn decode(&mut self, msg: Self::In) -> Result<Self::FrameIn> { Ok(msg) }
    async fn encode(&mut self, frame: Self::FrameOut) -> Result<Self::Out> { Ok(frame) }
}

#[derive(Debug, Clone)]
struct DummyProtocol;

#[async_trait::async_trait]
impl Protocol for DummyProtocol {
    type FrameIn = ();
    type FrameOut = ();

    async fn handshake(&self, _io: &mut abstractdatabase::protocol::ProtocolIo<'_, Self>) -> Result<()> {
        Ok(())
    }
    async fn ping(&self, _io: &mut abstractdatabase::protocol::ProtocolIo<'_, Self>) -> Result<()> {
        Ok(())
    }
    async fn reset(&self, _io: &mut abstractdatabase::protocol::ProtocolIo<'_, Self>) -> Result<()> {
        Ok(())
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

    // This replaces DummyFactory:
    let factory = ProtocolFactory {
        transport_factory: DummyTransportFactory,
        codec: PassthroughCodec,
        protocol: DummyProtocol,
        connect_timeout: Some(Duration::from_secs(2)),
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