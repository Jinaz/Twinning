use std::sync::Arc;
use std::sync::RwLock;

use dt_database::postgresimpl::PostgresDatabase;
use dt_gateway_fischertechnik::Gateway;
use dt_gateway_fischertechnik::GatewayConfig;
use dt_gateway_fischertechnik::GatewayEvent;
use dt_gateway_fischertechnik::downstreaminterface::MqttDownstreamConfig;
use dt_gateway_fischertechnik::models::properties::PropertyModel;
use dt_gateway_fischertechnik::upstreaminterface::UpstreamConfig;
use fischertechnikdtengine::eventsystem::eventqueue::EventQueue;
use fischertechnikdtengine::eventsystem::workflowengine::WorkflowEngine;
use fischertechnikdtengine::synchronizer::mappings::positionmapping::PositionMapping;
use fischertechnikdtengine::synchronizer::synchronizer::Synchronizer;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::init();

    let event_queue = EventQueue::new(1024);
    let workflow_engine = WorkflowEngine::new(event_queue.clone()).with_budget(32);

    // Gateway config (machine broker)
    let cfg = GatewayConfig {
        downstream: MqttDownstreamConfig {
            host: "127.0.0.1".into(),
            port: 1883,
            client_id: "sampleengine-ft-gw".into(),
            username: None,
            password: None,
            keep_alive_secs: 30,
            subscribe_filter: "fischertechnik/#".into(),
        },
        upstream: UpstreamConfig {
            command_prefix: "fischertechnik/commands".into(),
            structured_event_prefix: Some("fischertechnik/events".into()),
        },
        event_channel_capacity: 2048,
        command_channel_capacity: 256,
    };

    // Start gateway (this is the MQTT reader)
    let mut gateway = Gateway::start(cfg).await?;
    let cmd_tx = gateway.commands();
    // Property model lives in the engine (what you asked for)
    let props = Arc::new(RwLock::new(PropertyModel::default()));

    // Optional: periodic readout (shows that properties are being updated)
    
        let props = props.clone();
        tokio::spawn(async move {
            loop {
                {
                    let pm = props.read().await;

                    if let Some(s) = pm.speed.get("förderband1-4") {
                        info!("PROP speed förderband1-4 = {} {:?}", s.value, s.unit);
                    }
                    if let Some(p) = pm.position.get("förderband1-4") {
                        info!("PROP position förderband1-4 = {} {:?}", p.value, p.unit);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });

    let db = Arc::new(PostgresDatabase::new());

    let mapping = Box::new(
        PositionMapping::new("PositionMapping", gateway.clone(), db.clone())
    );
    let synchronizer = Synchronizer::new(vec![mapping]);

    // MAIN LOOP: "read from MQTT" (via gateway events) and "write to property value"
    while let Some(evt) = gateway.events().recv().await {
        match &evt {
            GatewayEvent::Telemetry(t) => {
                // This is the key line: write MQTT-derived telemetry into your property model
                props.write().await.apply_telemetry(t);

                // Example: if speed too high, send a command back
                if let Some(v) = match &t.value {
                    mqtt::models::events::Value::Number { v, .. } => Some(*v),
                    _ => None,
                } {
                    if t.topic.ends_with("/speed") && v > 1.0 {
                        // Use the property helper to create a command (optional)
                        let cmd: Command = {
                            let pm = props.read().await;
                            pm.speed.set_target_speed_cmd("förderband1-4", 0.7)
                        };
                        let _ = cmd_tx.send(cmd).await;
                    }
                }
            }

            GatewayEvent::MachineFailure(f) => {
                warn!("ENGINE: machine failure component='{}' reason={:?}", f.component, f.message);
                // you can react with commands here
            }

            GatewayEvent::CounterDeviation(d) => {
                warn!("ENGINE: counter deviation '{}' expected={} actual={}", d.counter_name, d.expected, d.actual);
            }
        }
    }
    Ok(())
}