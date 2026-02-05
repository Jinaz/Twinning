use clap::Parser;
use mqtt::{
    downstreaminterface::MqttDownstreamConfig,
    upstreaminterface::UpstreamConfig,
    Gateway, GatewayConfig,
};
use tracing::info;

#[derive(Parser, Debug, Clone)]
struct Args {
    #[arg(long, env="MQTT_HOST", default_value="127.0.0.1")]
    host: String,
    #[arg(long, env="MQTT_PORT", default_value_t=1883)]
    port: u16,
    #[arg(long, env="MQTT_CLIENT_ID", default_value="ft-gw-cli")]
    client_id: String,

    #[arg(long, env="MQTT_USER")]
    username: Option<String>,
    #[arg(long, env="MQTT_PASS")]
    password: Option<String>,

    #[arg(long, env="MQTT_SUB", default_value="fischertechnik/#")]
    subscribe_filter: String,

    /// Where commands should be published to on the machine broker
    #[arg(long, env="CMD_PREFIX", default_value="fischertechnik/commands")]
    command_prefix: String,

    /// Optional structured events prefix (set if machines publish JSON events there)
    #[arg(long, env="EVENT_PREFIX")]
    structured_event_prefix: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();

    let cfg = GatewayConfig {
        downstream: MqttDownstreamConfig {
            host: args.host,
            port: args.port,
            client_id: args.client_id,
            username: args.username,
            password: args.password,
            keep_alive_secs: 30,
            subscribe_filter: args.subscribe_filter,
        },
        upstream: UpstreamConfig {
            command_prefix: args.command_prefix,
            structured_event_prefix: args.structured_event_prefix,
        },
        event_channel_capacity: 1024,
        command_channel_capacity: 128,
    };

    let mut gw = Gateway::start(cfg).await?;
    info!("CLI running. Printing events as JSON...");

    while let Some(evt) = gw.events().recv().await {
        println!("{}", serde_json::to_string(&evt)?);
    }

    Ok(())
}
