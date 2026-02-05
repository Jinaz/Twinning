use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::downstreaminterface::{DownstreamInterface, MqttDownstream, MqttDownstreamConfig};
use crate::models::commands::Command;
use crate::models::events::GatewayEvent;
use crate::upstreaminterface::{DefaultUpstream, UpstreamConfig, UpstreamInterface};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::models::properties::PropertyModel;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub downstream: MqttDownstreamConfig,
    pub upstream: UpstreamConfig,
    pub event_channel_capacity: usize,
    pub command_channel_capacity: usize,
}

/// Gateway = (Downstream MQTT transport) + (Upstream translation) + (channels for engine)
pub struct Gateway {
    join: JoinHandle<Result<()>>,
    events_rx: mpsc::Receiver<GatewayEvent>,
    commands_tx: mpsc::Sender<Command>,
    shutdown: CancellationToken,
 properties: Arc<RwLock<PropertyModel>>,
}

impl Gateway {
    pub fn properties(&self) -> Arc<RwLock<PropertyModel>> {
        self.properties.clone()
    }
    pub fn events(&mut self) -> &mut mpsc::Receiver<GatewayEvent> {
        &mut self.events_rx
    }

    pub fn commands(&self) -> mpsc::Sender<Command> {
        self.commands_tx.clone()
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub async fn stop(self) -> Result<()> {
        self.shutdown.cancel();
        self.join.await.context("gateway join failed")?
    }

    pub async fn start(cfg: GatewayConfig) -> Result<Self> {
        let shutdown = CancellationToken::new();
        let (events_tx, events_rx) = mpsc::channel(cfg.event_channel_capacity);
        let (commands_tx, mut commands_rx) = mpsc::channel(cfg.command_channel_capacity);
        
        let properties = Arc::new(RwLock::new(PropertyModel::default()));
let properties_task = properties.clone();

        // build concrete downstream + upstream (your “interfaces”)
        let mut downstream = MqttDownstream::connect(cfg.downstream).await?;
        downstream.subscribe().await?;

        let upstream = DefaultUpstream::new(cfg.upstream);

        info!("Gateway started: subscribed to machine broker.");

        let task_shutdown = shutdown.clone();

        let join = tokio::spawn(async move {
            // Run loop: receive MQTT, decode to events, and publish commands
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => {
                        info!("Gateway shutdown requested.");
                        break;
                    }

                    cmd = commands_rx.recv() => {
                        match cmd {
                            Some(c) => {
                                let publish = upstream.encode_command(&c)?;
                                downstream.publish(publish).await?;
                            }
                            None => {
                                // engine dropped command channel
                                warn!("Command channel closed. Gateway continues receiving events.");
                            }
                        }
                    }

                    msg = downstream.recv() => {
                        let msg = match msg? {
                            Some(m) => m,
                            None => continue,
                        };

                        let evts = upstream.decode_events(&msg);

for evt in evts {
    // Update property model first (only telemetry matters here)
    if let GatewayEvent::Telemetry(t) = &evt {
        let mut pm = properties_task.write().await;
        pm.apply_telemetry(t);
    }

    if events_tx.send(evt).await.is_err() {
        warn!("Event receiver dropped. Stopping gateway task.");
        return Ok(());
    }
}

                    }
                }
            }

            Ok(())
        });

        Ok(Self {
    join,
    events_rx,
    commands_tx,
    shutdown,
    properties,
})

    }
}
