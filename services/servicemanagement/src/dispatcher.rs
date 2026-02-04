//! Non-RT Dispatcher
//!
//! Provides a dedicated worker thread for dispatching requests to Non-RT services.
//! Uses crossbeam bounded channels for communication.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use nonrtservices::NonRtService;

/// Request sent to the Non-RT dispatcher
pub struct NonRtRequest {
    pub service_name: String,
    pub req: serde_json::Value,
    pub reply: crossbeam_channel::Sender<Result<serde_json::Value, String>>,
}

/// Non-RT service dispatcher with a dedicated worker thread.
pub struct NonRtDispatcher {
    tx: crossbeam_channel::Sender<NonRtRequest>,
    _handle: std::thread::JoinHandle<()>,
}

impl NonRtDispatcher {
    /// Create a new dispatcher with the given channel capacity and service map.
    pub fn new(
        capacity: usize,
        services: Arc<RwLock<HashMap<String, Arc<RwLock<Box<dyn NonRtService>>>>>>,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<NonRtRequest>(capacity);

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime for NonRtDispatcher");

            rt.block_on(async move {
                while let Ok(request) = rx.recv() {
                    let svcs = services.read().await;
                    let result = match svcs.get(&request.service_name) {
                        Some(svc) => {
                            let svc = svc.read().await;
                            // NonRtService doesn't have a generic call() method,
                            // so we return the service status as JSON for now.
                            let status = svc.status();
                            match serde_json::to_value(status) {
                                Ok(v) => Ok(v),
                                Err(e) => Err(e.to_string()),
                            }
                        }
                        None => Err(format!("Service '{}' not found", request.service_name)),
                    };
                    let _ = request.reply.send(result);
                }
            });
        });

        Self {
            tx,
            _handle: handle,
        }
    }

    /// Send a request to a Non-RT service (blocking until reply).
    pub fn call(
        &self,
        service_name: &str,
        req: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let request = NonRtRequest {
            service_name: service_name.to_string(),
            req,
            reply: reply_tx,
        };
        self.tx
            .send(request)
            .map_err(|_| "Dispatcher channel closed".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "Reply channel closed".to_string())?
    }

    /// Try to send a request without blocking.
    pub fn try_call(
        &self,
        service_name: &str,
        req: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let request = NonRtRequest {
            service_name: service_name.to_string(),
            req,
            reply: reply_tx,
        };
        self.tx
            .try_send(request)
            .map_err(|_| "Dispatcher channel full or closed".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "Reply channel closed".to_string())?
    }
}
