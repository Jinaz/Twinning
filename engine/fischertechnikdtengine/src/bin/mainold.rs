use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::RwLock;

use fischertechnikdtengine::{
    eventsystem::{
        eventqueue::{Event, EventKind, EventQueue, EventSource},
        workflowengine::WorkflowEngine,
    },
    synchronizer::{
        mappings::positionmapping::{DatabasePort, GatewayPort, MappingIoError, Position, PositionMapping},
        synchronizer::Synchronizer,
    },
};

// ----- Mock Gateway -----
struct MockGateway {
    pos: RwLock<Position>,
}

impl MockGateway {
    fn new() -> Self {
        Self {
            pos: RwLock::new(Position { x: 0.0, y: 0.0, z: 0.0 }),
        }
    }

    async fn simulate_move(&self) {
        let mut g = self.pos.write().await;
        g.x += 0.1;
        g.y += 0.2;
        g.z += 0.05;
    }
}

#[async_trait]
impl GatewayPort for MockGateway {
    async fn get_position(&self) -> Result<Position, MappingIoError> {
        Ok(*self.pos.read().await)
    }
}

// ----- Mock DB -----
struct MockDatabase {
    stored: RwLock<Option<Position>>,
}

impl MockDatabase {
    fn new() -> Self {
        Self { stored: RwLock::new(None) }
    }
}

impl DatabasePort for MockDatabase {
    fn try_set_position(&self, pos: Position) -> Result<(), MappingIoError> {
        // Non-awaiting API requirement: we must not .await here.
        // For the mock, we can use try_write (immediate), otherwise drop.
        match self.stored.try_write() {
            Ok(mut g) => {
                *g = Some(pos);
                Ok(())
            }
            Err(_) => Err(MappingIoError::DbWrite("mockdb busy (dropped)".into())),
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let event_queue = EventQueue::new(1024);
    let workflow_engine = WorkflowEngine::new(event_queue.clone()).with_budget(32);

    let gateway = Arc::new(MockGateway::new());
    let db = Arc::new(MockDatabase::new());

    let mapping = Box::new(PositionMapping::new("PositionMapping", gateway.clone(), db.clone()));
    let synchronizer = Synchronizer::new(vec![mapping]);

    // Producer: simulate gateway changes + enqueue events
    tokio::spawn({
        let q = event_queue.clone();
        let gw = gateway.clone();
        async move {
            loop {
                gw.simulate_move().await;

                // RT-friendly: try_push (no await). If full/busy, drop.
                let _ = q.try_push(Event::new(EventKind::PositionChanged, EventSource::Gateway));

                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });

    log::info!("engine started. Ctrl+C to stop.");

    // Main engine loop: synchronizer then workflow engine
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("shutdown requested");
                break;
            }

            _ = async {
                synchronizer.tick().await;
                workflow_engine.tick().await;
                tokio::time::sleep(Duration::from_millis(10)).await; // engine tick
            } => {
            }
        }
    }

    Ok(())
}
