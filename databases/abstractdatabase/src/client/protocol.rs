// src/protocol.rs
use anyhow::{Context, Result};
use std::{fmt::Debug, sync::Arc};
use std::time::Duration;
use async_trait::async_trait;

use crate::client::connector::{ConnectionFactory, ManagedConnection}; // adjust path to your traits

#[async_trait::async_trait]
pub trait Io {
    type FrameIn;
    type FrameOut;
    async fn send(&mut self, f: Self::FrameOut) -> Result<()>;
    async fn recv(&mut self) -> Result<Self::FrameIn>;
}

#[async_trait::async_trait]
pub trait ProtocolIo: Send + Sync + Debug {
    type FrameIn: Send + Debug + 'static;
    type FrameOut: Send + Debug + 'static;

    async fn send(&mut self, frame: Self::FrameOut) -> Result<()>;
    async fn recv(&mut self) -> Result<Self::FrameIn>;
}


/// A minimal transport abstraction.
/// Implement this for TcpStream-based protocols, HTTP clients, WebSocket streams, etc.
///
/// If you already have your own transport abstraction, replace this trait accordingly.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + Debug + 'static {
    type Inbound: Send + Debug + 'static;
    type Outbound: Send + Debug + 'static;

    async fn send(&mut self, msg: Self::Outbound) -> Result<()>;
    async fn recv(&mut self) -> Result<Self::Inbound>;
    async fn close(&mut self) -> Result<()>;
}

/// A codec maps between raw transport messages and higher-level protocol frames.
/// For some protocols, Transport messages *are* frames already → codec is trivial.
#[async_trait::async_trait]
pub trait Codec: Send + Sync + Debug + 'static {
    type In: Send + Debug + 'static;
    type Out: Send + Debug + 'static;

    type FrameIn: Send + Debug + 'static;
    type FrameOut: Send + Debug + 'static;

    async fn decode(&mut self, msg: Self::In) -> Result<Self::FrameIn>;
    async fn encode(&mut self, frame: Self::FrameOut) -> Result<Self::Out>;
}

/// Protocol behavior hooks:
/// - handshake/auth
/// - ping/health
/// - reset (rollback tx, clear session vars, etc.)
///
/// This keeps DB-specific behavior out of the connector pool.
#[async_trait::async_trait]
pub trait Protocol: Send + Sync + Debug +Sized+ 'static {
    type FrameIn: Send + Debug + 'static;
    type FrameOut: Send + Debug + 'static;

    /// Called once on fresh connection.
    async fn handshake(&self, io: &mut dyn ProtocolIo<FrameIn=Self::FrameIn, FrameOut=Self::FrameOut>) -> Result<()>;

    /// Lightweight health check (ping or equivalent).
    async fn ping(&self, io: &mut dyn ProtocolIo<FrameIn=Self::FrameIn, FrameOut=Self::FrameOut>) -> Result<()>;

    /// Reset session state when returned to pool.
    async fn reset(&self, io: &mut dyn ProtocolIo<FrameIn=Self::FrameIn, FrameOut=Self::FrameOut>) -> Result<()>;

    /// Optional: close hook (e.g., send quit frame). Default is noop.
    async fn on_close(&self, _io: &mut dyn ProtocolIo<FrameIn=Self::FrameIn, FrameOut=Self::FrameOut>) -> Result<()>;
}



/// A concrete connection that the connector pool can manage:
/// combines Transport + Codec + Protocol.
///
/// - T: how bytes/messages move
/// - C: how messages map to frames
/// - P: protocol semantics (handshake/ping/reset)
#[derive(Debug)]
pub struct ProtocolConnection<T, C, P>
where
    T: Transport,
    C: Codec<In = T::Inbound, Out = T::Outbound>,
    P: Protocol<FrameIn = C::FrameIn, FrameOut = C::FrameOut>,
{
    transport: T,
    codec: C,
    protocol: Arc<P>,
    healthy: bool,
}

impl<T, C, P> ProtocolConnection<T, C, P>
where
    T: Transport,
    C: Codec<In = T::Inbound, Out = T::Outbound>,
    P: Protocol<FrameIn = C::FrameIn, FrameOut = C::FrameOut>,
{
    pub fn new(transport: T, codec: C, protocol: Arc<P>) -> Self {
        Self {
            transport,
            codec,
            protocol,
            healthy: true,
        }
    }

    /// Low-level: send a protocol frame.
    pub async fn send_frame(&mut self, frame: P::FrameOut) -> Result<()> {
        let msg = self.codec.encode(frame).await?;
        self.transport.send(msg).await
    }
    /// Receive a protocol frame. 
    async fn recv_frame(&mut self) -> Result<P::FrameIn> { 
        let msg = self.transport.recv().await?; 
        self.codec.decode(msg).await
    }

    pub async fn handshake(&mut self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol); // clone handle (no borrow of self across await)
        protocol.handshake(self).await
    }

    pub async fn ping(&mut self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        protocol.ping(self).await
    }

    pub async fn reset_session(&mut self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        protocol.reset(self).await
    }

    pub async fn close_gracefully(&mut self) -> Result<()> {
        let protocol = Arc::clone(&self.protocol);
        let _ = protocol.on_close(self).await;
        self.transport.close().await
    }
}

/// ProtocolConnection is the ProtocolIo
#[async_trait]
impl<T, C, P> ProtocolIo for ProtocolConnection<T, C, P>
where
    T: Transport,
    C: Codec<In = T::Inbound, Out = T::Outbound>,
    P: Protocol<FrameIn = C::FrameIn, FrameOut = C::FrameOut>,
{
    type FrameIn = P::FrameIn;
    type FrameOut = P::FrameOut;

    async fn send(&mut self, frame: Self::FrameOut) -> Result<()> {
        self.send_frame(frame).await
    }

    async fn recv(&mut self) -> Result<Self::FrameIn> {
        self.recv_frame().await
    }
}

/// Make ProtocolConnection usable by the generic connector pool.
#[async_trait::async_trait]
impl<T, C, P> ManagedConnection for ProtocolConnection<T, C, P>
where
    T: Transport,
    C: Codec<In = T::Inbound, Out = T::Outbound>,
    P: Protocol<FrameIn = C::FrameIn, FrameOut = C::FrameOut>,
{
    async fn is_healthy(&mut self) -> bool {
        if !self.healthy {
            return false;
        }
        let ok = self.ping().await.is_ok();
        self.healthy = ok;
        ok
    }

    async fn reset(&mut self) -> Result<()> {
        // Reset session state; if it fails mark unhealthy so pool discards it.
        if let Err(e) = self.reset_session().await {
            self.healthy = false;
            return Err(e);
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        // Best-effort protocol close hook isn't called in this minimal version.
        self.transport.close().await
    }
}

/// A generic protocol factory that produces a pooled ProtocolConnection.
/// This is what you pass into your Connector<...>.
#[derive(Debug, Clone)]
pub struct ProtocolFactory<TF, C, P> {
    pub cfg: Arc<crate::model::configuration::ClientConfig>,
    pub transport_factory: TF,
    pub codec: C,
    pub protocol: Arc<P>,

    /// Optional: timeout for establishing the connection (transport_factory.connect()).
    pub connect_timeout: Option<Duration>,
}

#[async_trait::async_trait]
pub trait TransportFactory: Send + Sync + Debug + 'static {
    type T: Transport;
    async fn connect(&self, cfg: &crate::model::configuration::ClientConfig) -> Result<Self::T>;
}

#[async_trait::async_trait]
impl<TF, C, P> ConnectionFactory for ProtocolFactory<TF, C, P>
where
    TF: TransportFactory,
    C: Codec<In = <TF::T as Transport>::Inbound, Out = <TF::T as Transport>::Outbound> + Clone,
    P: Protocol<FrameIn = C::FrameIn, FrameOut = C::FrameOut>,
{
    type Conn = ProtocolConnection<TF::T, C, P>;

    async fn connect(&self) -> Result<Self::Conn> {
        //let make_transport = self.transport_factory.connect();
         let transport = self.transport_factory.connect(&self.cfg).await?;
        //let transport = if let Some(t) = self.connect_timeout {
        //    tokio::time::timeout(t, make_transport)
        //        .await
        //        .context("transport connect timed out")??
        //} else {
        //    make_transport.await?
        //};

        let mut conn = ProtocolConnection::new(transport, self.codec.clone(), Arc::clone(&self.protocol));

        // Here is where you'd call conn.protocol.handshake(...) if you choose the ProtocolIo approach.
        // For now, leave handshake to the DB-specific constructor, or evolve Protocol trait to accept &mut ProtocolConnection.
        //
        // e.g. conn.protocol.handshake_conn(&mut conn).await?;
        conn.handshake().await?;
        Ok(conn)
    }
}
