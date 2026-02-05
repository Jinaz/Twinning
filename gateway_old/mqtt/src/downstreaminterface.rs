use anyhow::{Context, Result};
use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS};
use std::time::Duration;
use tracing::warn;

use crate::models::types::{MqttMessage, MqttPublish, Qos};

#[derive(Debug, Clone)]
pub struct MqttDownstreamConfig {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub keep_alive_secs: u64,
    pub subscribe_filter: String,
}

fn map_qos(q: &Qos) -> QoS {
    match q {
        Qos::AtMostOnce => QoS::AtMostOnce,
        Qos::AtLeastOnce => QoS::AtLeastOnce,
        Qos::ExactlyOnce => QoS::ExactlyOnce,
    }
}

#[async_trait]
pub trait DownstreamInterface: Send {
    async fn subscribe(&mut self) -> Result<()>;
    async fn recv(&mut self) -> Result<Option<MqttMessage>>;
    async fn publish(&mut self, msg: MqttPublish) -> Result<()>;
}

pub struct MqttDownstream {
    cfg: MqttDownstreamConfig,
    client: AsyncClient,
    eventloop: rumqttc::EventLoop,
}

impl MqttDownstream {
    pub async fn connect(cfg: MqttDownstreamConfig) -> Result<Self> {
        let mut opts = MqttOptions::new(&cfg.client_id, &cfg.host, cfg.port);
        opts.set_keep_alive(Duration::from_secs(cfg.keep_alive_secs));

        if let (Some(u), Some(p)) = (cfg.username.clone(), cfg.password.clone()) {
            opts.set_credentials(u, p);
        }

        let (client, eventloop) = AsyncClient::new(opts, 50);
        Ok(Self { cfg, client, eventloop })
    }
}

#[async_trait]
impl DownstreamInterface for MqttDownstream {
    async fn subscribe(&mut self) -> Result<()> {
        self.client
            .subscribe(&self.cfg.subscribe_filter, QoS::AtLeastOnce)
            .await
            .with_context(|| format!("subscribe failed for '{}'", self.cfg.subscribe_filter))?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<MqttMessage>> {
        loop {
            match self.eventloop.poll().await {
                Ok(Event::Incoming(Incoming::Publish(p))) => {
                    return Ok(Some(MqttMessage {
                        topic: p.topic,
                        payload: p.payload.to_vec(),
                    }));
                }
                Ok(_) => continue,
                Err(e) => {
                    warn!("MQTT poll error: {:?} (retrying)", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    // keep trying
                }
            }
        }
    }

    async fn publish(&mut self, msg: MqttPublish) -> Result<()> {
        self.client
            .publish(msg.topic, map_qos(&msg.qos), msg.retain, msg.payload)
            .await
            .context("publish failed")?;
        Ok(())
    }
}
