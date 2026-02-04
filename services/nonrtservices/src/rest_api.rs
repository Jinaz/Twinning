//! REST API Service using Axum
//!
//! Provides HTTP endpoints for service management and property access:
//! - GET /health - Health check
//! - GET /services - List all services
//! - GET /services/{name}/status - Get service status
//! - POST /services/{name}/start - Start a service
//! - POST /services/{name}/stop - Stop a service
//! - GET /properties/speed - Get current speed
//! - PUT /properties/speed - Set speed
//! - POST /services/{name} - Call a Non-RT service
//! - POST /events - Inject an engine event

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tokio::sync::RwLock;

use rtservices::{EngineEvent, Properties};

use crate::traits::{NonRtService, ServiceResult, ServiceStatus};

/// REST API state shared across handlers
pub struct ApiState {
    services: RwLock<HashMap<String, Arc<RwLock<Box<dyn NonRtService>>>>>,
    /// Shared atomic properties (optional, set when engine is connected)
    pub properties: Option<Arc<Properties>>,
    /// Event sender (optional, set when engine is connected)
    pub event_tx: Option<crossbeam_channel::Sender<EngineEvent>>,
}

impl ApiState {
    pub fn new() -> Self {
        Self {
            services: RwLock::new(HashMap::new()),
            properties: None,
            event_tx: None,
        }
    }

    pub fn with_properties(mut self, properties: Arc<Properties>) -> Self {
        self.properties = Some(properties);
        self
    }

    pub fn with_event_tx(mut self, tx: crossbeam_channel::Sender<EngineEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub async fn register_service(&self, service: Box<dyn NonRtService>) {
        let name = service.name().to_string();
        let mut services = self.services.write().await;
        services.insert(name, Arc::new(RwLock::new(service)));
    }

    pub async fn get_service(&self, name: &str) -> Option<Arc<RwLock<Box<dyn NonRtService>>>> {
        let services = self.services.read().await;
        services.get(name).cloned()
    }
}

impl Default for ApiState {
    fn default() -> Self {
        Self::new()
    }
}

/// API response types
#[derive(serde::Serialize)]
struct ServiceListResponse {
    services: Vec<String>,
    count: usize,
}

#[derive(serde::Serialize)]
struct HealthResponse {
    status: String,
    timestamp: String,
}

#[derive(serde::Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SpeedPayload {
    value: f64,
}

#[derive(serde::Deserialize)]
struct EventRequest {
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
}

/// List all registered services
async fn list_services(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    let services = state.services.read().await;
    let names: Vec<String> = services.keys().cloned().collect();
    let count = names.len();

    Json(ServiceListResponse {
        services: names,
        count,
    })
}

/// Get status of a specific service
async fn get_service_status(
    State(state): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.get_service(&name).await {
        Some(service) => {
            let svc = service.read().await;
            let status = svc.status();
            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Service '{}' not found", name),
            }),
        )
            .into_response(),
    }
}

/// Start a service
async fn start_service(
    State(state): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.get_service(&name).await {
        Some(service) => {
            let mut svc = service.write().await;
            match svc.start().await {
                Ok(()) => {
                    let status = svc.status();
                    (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Service '{}' not found", name),
            }),
        )
            .into_response(),
    }
}

/// Stop a service
async fn stop_service(
    State(state): State<Arc<ApiState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.get_service(&name).await {
        Some(service) => {
            let mut svc = service.write().await;
            match svc.stop().await {
                Ok(()) => {
                    let status = svc.status();
                    (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Service '{}' not found", name),
            }),
        )
            .into_response(),
    }
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// GET /properties/speed - Read current speed
async fn get_speed(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    match &state.properties {
        Some(props) => {
            let value = props.get_speed();
            (StatusCode::OK, Json(SpeedPayload { value })).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Properties not available".to_string(),
            }),
        )
            .into_response(),
    }
}

/// PUT /properties/speed - Set speed
async fn set_speed(
    State(state): State<Arc<ApiState>>,
    Json(payload): Json<SpeedPayload>,
) -> impl IntoResponse {
    match &state.properties {
        Some(props) => {
            props.set_speed(payload.value);
            (StatusCode::OK, Json(SpeedPayload { value: payload.value })).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Properties not available".to_string(),
            }),
        )
            .into_response(),
    }
}

/// POST /services/{name} - Call a Non-RT service
async fn call_service(
    State(state): State<Arc<ApiState>>,
    Path(name): Path<String>,
    Json(_body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match state.get_service(&name).await {
        Some(service) => {
            let svc = service.read().await;
            let status = svc.status();
            (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Service '{}' not found", name),
            }),
        )
            .into_response(),
    }
}

/// POST /events - Inject an engine event
async fn post_event(
    State(state): State<Arc<ApiState>>,
    Json(req): Json<EventRequest>,
) -> impl IntoResponse {
    match &state.event_tx {
        Some(tx) => {
            let event = match req.kind.as_str() {
                "SetSpeed" => {
                    let value = req.payload.as_f64().unwrap_or(0.0);
                    EngineEvent::SetSpeed(value)
                }
                _ => EngineEvent::Custom {
                    topic: req.kind,
                    payload: req.payload,
                },
            };

            match tx.try_send(event) {
                Ok(()) => (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "accepted"})),
                )
                    .into_response(),
                Err(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: "Event channel full".to_string(),
                    }),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Event channel not available".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Create the REST API router
pub fn create_router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/services", get(list_services))
        .route("/services/:name/status", get(get_service_status))
        .route("/services/:name/start", post(start_service))
        .route("/services/:name/stop", post(stop_service))
        .route("/services/:name", post(call_service))
        .route("/properties/speed", get(get_speed).put(set_speed))
        .route("/events", post(post_event))
        .with_state(state)
}

/// REST API Service
pub struct RestApiService {
    name: String,
    bind_addr: String,
    state: Arc<ApiState>,
    status: ServiceStatus,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RestApiService {
    pub fn new(bind_addr: &str) -> Self {
        Self {
            name: "rest_api".to_string(),
            bind_addr: bind_addr.to_string(),
            state: Arc::new(ApiState::new()),
            status: ServiceStatus::new("rest_api"),
            shutdown_tx: None,
        }
    }

    pub fn with_state(mut self, state: Arc<ApiState>) -> Self {
        self.state = state;
        self
    }

    pub fn state(&self) -> Arc<ApiState> {
        self.state.clone()
    }

    pub async fn register_service(&self, service: Box<dyn NonRtService>) {
        self.state.register_service(service).await;
    }

    /// Reload / refresh the REST API service (placeholder for hot-reload).
    pub fn reload(&mut self) {
        log::info!("REST API service reload requested (no-op placeholder)");
    }
}

#[async_trait::async_trait]
impl crate::traits::NonRtService for RestApiService {
    fn name(&self) -> &str {
        &self.name
    }

    async fn start(&mut self) -> ServiceResult<()> {
        if self.status.running {
            return Err(crate::traits::ServiceError::AlreadyRunning);
        }

        let router = create_router(self.state.clone());
        let bind_addr = self.bind_addr.clone();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Parse address
        let addr: std::net::SocketAddr = bind_addr
            .parse()
            .map_err(|e| crate::traits::ServiceError::StartFailed(format!("Invalid address: {}", e)))?;

        // Spawn the server
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            log::info!("REST API listening on {}", addr);

            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                    log::info!("REST API shutting down");
                })
                .await
                .ok();
        });

        self.status.running = true;
        self.status.started_at = Some(chrono::Utc::now());

        log::info!("REST API service started on {}", self.bind_addr);
        Ok(())
    }

    async fn stop(&mut self) -> ServiceResult<()> {
        if !self.status.running {
            return Err(crate::traits::ServiceError::NotRunning);
        }

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        self.status.running = false;
        log::info!("REST API service stopped");
        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.status.running
    }

    fn status(&self) -> ServiceStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_api_state() {
        let state = ApiState::new();

        // Create a mock service
        struct MockService {
            name: String,
            status: ServiceStatus,
        }

        #[async_trait::async_trait]
        impl crate::traits::NonRtService for MockService {
            fn name(&self) -> &str {
                &self.name
            }
            async fn start(&mut self) -> ServiceResult<()> {
                Ok(())
            }
            async fn stop(&mut self) -> ServiceResult<()> {
                Ok(())
            }
            async fn health_check(&self) -> bool {
                true
            }
            fn status(&self) -> ServiceStatus {
                self.status.clone()
            }
        }

        let mock = MockService {
            name: "mock".to_string(),
            status: ServiceStatus::new("mock"),
        };

        state.register_service(Box::new(mock)).await;

        let service = state.get_service("mock").await;
        assert!(service.is_some());
    }
}
