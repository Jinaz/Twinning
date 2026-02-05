use abstractgateway::{Gateway, GatewayApi};
use abstractgateway::downstream::{Configuration, Method, Property};
use abstractgateway::downstream::configuration::Id;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use abstractgateway::{IUpstream, IDownstream};


pub struct MqttGateway {
    pub core: Gateway,                 // <-- this is your "base class"
    client: AsyncClient,
    event_task: JoinHandle<()>,
}

impl MqttGateway {
    pub async fn connect(core: Gateway, host: &str, port: u16, client_id: &str) -> Result<Self> {
        let mut opts = MqttOptions::new(client_id, host, port);

        // Pull credentials from downstream config if present
        if let Some(conf) = core.config.downstream.read().await.configuration().cloned() {
            if !conf.username.is_empty() {
                opts.set_credentials(conf.username, conf.password);
            }
        }

        opts.set_keep_alive(Duration::from_secs(30));

        let (client, mut event_loop) = AsyncClient::new(opts, 10);

        // Example subscriptions (adjust to your topic model)
        client.subscribe("dt/+/event/+", QoS::AtLeastOnce).await?;
        client.subscribe("dt/+/property/+", QoS::AtLeastOnce).await?;

        // Clone what we need into the task
        let cancel = core.cancellation_token();
        let downstream = core.config.downstream.clone();
        let upstream = core.config.upstream.clone();

        let event_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("MQTT loop cancelled");
                        break;
                    }
                    ev = event_loop.poll() => {
                        match ev {
                            Ok(Event::Incoming(Packet::Publish(p))) => {
                                // TODO: decode topic/payload and update state:
                                // - property update -> downstream.write().await.upsert_property(...)
                                // - event -> upstream.events.publish(...)
                                let _ = (&downstream, &upstream, p);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                warn!("MQTT poll error: {e}");
                                tokio::time::sleep(Duration::from_millis(250)).await;
                            }
                        }
                    }
                }
            }
        });

        Ok(Self { core, client, event_task })
    }
}

#[async_trait]
impl GatewayApi for MqttGateway {
    async fn configure_config(&self, conf: Configuration) -> Result<()> {
        self.core.configure_config(conf).await
    }

    async fn configure_property(&self, id: Id, prop: Property) -> Result<()> {
        self.core.configure_property(id, prop).await
    }

    async fn configure_method(&self, id: Id, method: Method) -> Result<()> {
        self.core.configure_method(id, method).await
    }

    async fn execute_method(&self, name: &Id) -> Result<()> {
        // MQTT-specific execution: publish request
        let ds = self.core.config.downstream.read().await;
        let method = ds.method(name).cloned().context("unknown method id")?;

        let payload = serde_json::to_vec(&method.variables)?;
        let topic = format!("dt/method/{name}/execute");

        self.client.publish(topic, QoS::AtLeastOnce, false, payload).await?;
        Ok(())
    }

    async fn run(&self) -> Result<()> {
        self.core.run().await
    }

    fn shutdown(&self) {
        self.core.shutdown()
    }
}


#[tokio::main]
async fn main() -> Result<()> {
    // Logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    // ---- Build downstream state (models/config) ----
    let mut downstream = IDownstream::new();
    downstream.set_configuration(Configuration {
        ipaddress: "127.0.0.1".to_string(),
        username: "user".to_string(),
        password: "pass".to_string(),
        settings: Default::default(),
    });

    // ---- Build upstream (event bus etc.) ----
    let upstream = IUpstream::new(1024);

    // ---- Core gateway ----
    let core = Gateway::new(downstream, upstream);

    // ---- Concrete MQTT gateway ----
    // Adjust host/port/client_id as needed
    let mqtt = MqttGateway::connect(core, "127.0.0.1", 1883, "dt-gateway-dev").await?;

    info!("MQTT gateway connected. Press Ctrl+C to stop.");

    // Wait for Ctrl+C then shutdown
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C received, shutting down...");
            mqtt.shutdown();
        }
        res = mqtt.run() => {
            // If your run() returns (e.g., due to cancellation), we log it.
            res?;
        }
    }

    info!("Gateway stopped.");
    Ok(())
}