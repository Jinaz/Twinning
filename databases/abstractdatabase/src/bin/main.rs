
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
use abstractdatabase::model::configuration::{ClientConfig, AuthKind, TopologyConfig, EndpointConfig};

use abstractdatabase::client::protocol::{
    Codec, Protocol, ProtocolFactory, Transport, TransportFactory,ProtocolIo
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
struct DummyTransport;

#[async_trait::async_trait]
impl Transport for DummyTransport {
    type Inbound = ();
    type Outbound = ();

    async fn send(&mut self, _msg: Self::Outbound) -> Result<()> {
        Ok(())
    }
    async fn recv(&mut self) -> Result<Self::Inbound> {
        Ok(())
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Now: TransportFactory does NOT need to store cfg.
/// It receives cfg in connect(&ClientConfig).
#[derive(Debug, Clone)]
struct DummyTransportFactory;

#[async_trait::async_trait]
impl TransportFactory for DummyTransportFactory {
    type T = DummyTransport;

    async fn connect(&self, cfg: &ClientConfig) -> Result<Self::T> {
        // Use cfg here (endpoint selection, tls/auth decisions, etc.)
        let ep = &cfg.topology.endpoints[0];
        println!("Connecting to {}:{}", ep.host, ep.port);

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

    async fn decode(&mut self, msg: Self::In) -> Result<Self::FrameIn> {
        Ok(msg)
    }

    async fn encode(&mut self, frame: Self::FrameOut) -> Result<Self::Out> {
        Ok(frame)
    }
}

#[derive(Debug)]
struct DummyProtocol;

#[async_trait::async_trait]
impl Protocol for DummyProtocol {
    type FrameIn = ();
    type FrameOut = ();

    async fn handshake(
        &self,
        _io: &mut dyn ProtocolIo<FrameIn = Self::FrameIn, FrameOut = Self::FrameOut>,
    ) -> Result<()> {
        Ok(())
    }

    async fn ping(
        &self,
        _io: &mut dyn ProtocolIo<FrameIn = Self::FrameIn, FrameOut = Self::FrameOut>,
    ) -> Result<()> {
        Ok(())
    }

    async fn reset(
        &self,
        _io: &mut dyn ProtocolIo<FrameIn = Self::FrameIn, FrameOut = Self::FrameOut>,
    ) -> Result<()> {
        Ok(())
    }

    async fn on_close(
        &self,
         _io: &mut dyn ProtocolIo<FrameIn=Self::FrameIn, FrameOut=Self::FrameOut>
        ) -> Result<()>{
        Ok(())
    }
}


fn connector_config_from_client(cfg: &ClientConfig) -> ConnectorConfig {
    ConnectorConfig {
        max_size: cfg.pooling.max_size,
        min_size: cfg.pooling.min_size,
        checkout_timeout: cfg.pooling.checkout_timeout,
        max_idle: cfg.pooling.max_idle,
        prewarm_interval: Duration::from_secs(10), // or add to PoolingConfig
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1) Build the unified config
    let client_cfg = ClientConfig {
        application_name: Some("demo".into()),
        topology: TopologyConfig {
            endpoints: vec![EndpointConfig::new("127.0.0.1", 5432)],
            database: Some("example".into()),
            ..Default::default()
        },
        ..Default::default()
    };

    client_cfg.validate()?;
    let client_cfg = Arc::new(client_cfg);

    // 2) Derive connector config
    let connector_cfg = connector_config_from_client(&client_cfg);

    // 3) Build protocol factory WITH cfg
    let factory = ProtocolFactory {
        cfg: Arc::clone(&client_cfg),               // <-- NEW in option B
        transport_factory: DummyTransportFactory,   // <-- no cfg inside
        codec: PassthroughCodec,
        protocol: Arc::new(DummyProtocol),
        connect_timeout: Some(Duration::from_secs(2)),
    };

    // 4) Create connector using protocol factory
    let connector = Connector::new(factory, connector_cfg);

    // 5) Use it
    let mut s = connector.session().await?;
    {
        let conn = s.conn_mut()?;
        let ok = conn.is_healthy().await;
        println!("healthy: {ok}");
    }
    drop(s);

    Ok(())
}