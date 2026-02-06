// src/configuration.rs
//! Generic database client configuration model.
//!
//! Goals:
//! - Database-agnostic: works for SQL/NoSQL/HTTP-based "databases" (InfluxDB, etc.)
//! - Explicit separation: endpoint/topology, auth, TLS, pooling, timeouts, session policy
//! - Validation helpers (no external deps)
//!
//! Optional integrations (behind feature flags you can add later):
//! - serde Serialize/Deserialize for config files
//! - url parsing (e.g., `url` crate) for DSN normalization

use std::{
    collections::BTreeMap,
    fmt::Debug,
    time::Duration,
};

/// High-level config for a client/connector.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub application_name: Option<String>,
    pub topology: TopologyConfig,
    pub auth: AuthConfig,
    pub tls: TlsConfig,
    pub pooling: PoolingConfig,
    pub timeouts: TimeoutConfig,
    pub session: SessionConfig,
    pub observability: ObservabilityConfig,

    /// Driver-/database-specific free-form options.
    /// Example: `{"search_path":"public","application_region":"eu"}`.
    pub options: BTreeMap<String, String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            application_name: None,
            topology: TopologyConfig::default(),
            auth: AuthConfig::default(),
            tls: TlsConfig::default(),
            pooling: PoolingConfig::default(),
            timeouts: TimeoutConfig::default(),
            session: SessionConfig::default(),
            observability: ObservabilityConfig::default(),
            options: BTreeMap::new(),
        }
    }
}

impl ClientConfig {
    /// Validate cross-field consistency. Keep this strict to fail fast.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.topology.validate()?;
        self.auth.validate()?;
        self.tls.validate()?;
        self.pooling.validate()?;
        self.timeouts.validate()?;
        self.session.validate()?;
        self.observability.validate()?;

        // Example consistency checks:
        if matches!(self.auth.kind, AuthKind::MutualTls) && !self.tls.enabled {
            return Err(ConfigError::Invalid(
                "auth=mTLS requires tls.enabled=true".into(),
            ));
        }

        Ok(())
    }

    /// Ergonomic builder start.
    pub fn builder() -> ClientConfigBuilder {
        ClientConfigBuilder::new()
    }
}

/// Builder for `ClientConfig` (avoids a “giant constructor”).
#[derive(Debug, Default)]
pub struct ClientConfigBuilder {
    cfg: ClientConfig,
}

impl ClientConfigBuilder {
    pub fn new() -> Self {
        Self {
            cfg: ClientConfig::default(),
        }
    }

    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.cfg.application_name = Some(name.into());
        self
    }

    pub fn topology(mut self, t: TopologyConfig) -> Self {
        self.cfg.topology = t;
        self
    }

    pub fn auth(mut self, a: AuthConfig) -> Self {
        self.cfg.auth = a;
        self
    }

    pub fn tls(mut self, t: TlsConfig) -> Self {
        self.cfg.tls = t;
        self
    }

    pub fn pooling(mut self, p: PoolingConfig) -> Self {
        self.cfg.pooling = p;
        self
    }

    pub fn timeouts(mut self, t: TimeoutConfig) -> Self {
        self.cfg.timeouts = t;
        self
    }

    pub fn session(mut self, s: SessionConfig) -> Self {
        self.cfg.session = s;
        self
    }

    pub fn observability(mut self, o: ObservabilityConfig) -> Self {
        self.cfg.observability = o;
        self
    }

    pub fn option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.cfg.options.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> Result<ClientConfig, ConfigError> {
        self.cfg.validate()?;
        Ok(self.cfg)
    }
}

/// -------------------------
/// Topology (endpoints, routing)
/// -------------------------

#[derive(Clone, Debug)]
pub struct TopologyConfig {
    /// One or more endpoints. For clustered DBs, add multiple.
    pub endpoints: Vec<EndpointConfig>,

    /// How a client should choose an endpoint.
    pub routing: RoutingPolicy,

    /// Optional: connect to a specific database/namespace on the server.
    pub database: Option<String>,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![EndpointConfig::localhost()],
            routing: RoutingPolicy::PrimaryOnly,
            database: None,
        }
    }
}

impl TopologyConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.endpoints.is_empty() {
            return Err(ConfigError::Invalid(
                "topology.endpoints must not be empty".into(),
            ));
        }
        for ep in &self.endpoints {
            ep.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct EndpointConfig {
    pub host: String,
    pub port: u16,

    /// Optional labels (region/az/role). Useful for smart routing.
    pub labels: BTreeMap<String, String>,
}

impl EndpointConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            labels: BTreeMap::new(),
        }
    }

    pub fn localhost() -> Self {
        Self::new("127.0.0.1", 0)
    }

    pub fn label(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.labels.insert(k.into(), v.into());
        self
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.host.trim().is_empty() {
            return Err(ConfigError::Invalid("endpoint.host is empty".into()));
        }
        // port can be 0 if transport resolves default (e.g., via scheme); allow but warn?
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum RoutingPolicy {
    /// Always connect to the first endpoint (or designated primary).
    PrimaryOnly,

    /// Round-robin across endpoints.
    RoundRobin,

    /// Prefer endpoints with a matching label (e.g., region=eu) else fallback.
    PreferLabel { key: String, value: String },

    /// Caller selects; library exposes endpoints but doesn't choose.
    Manual,
}

/// -------------------------
/// Authentication
/// -------------------------

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub kind: AuthKind,

    /// Optional user/identity (for display or certain auth modes).
    pub user: Option<String>,

    /// Optional password/token.
    /// In production you often load this from env/secret manager, not inline config.
    pub secret: Option<Secret>,

    /// Optional additional auth parameters.
    pub params: BTreeMap<String, String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            kind: AuthKind::None,
            user: None,
            secret: None,
            params: BTreeMap::new(),
        }
    }
}

impl AuthConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        match self.kind {
            AuthKind::None => Ok(()),
            AuthKind::UserPassword => {
                if self.user.as_deref().unwrap_or("").is_empty() {
                    return Err(ConfigError::Invalid("auth.user is required".into()));
                }
                if self.secret.is_none() {
                    return Err(ConfigError::Invalid("auth.secret is required".into()));
                }
                Ok(())
            }
            AuthKind::Token => {
                if self.secret.is_none() {
                    return Err(ConfigError::Invalid("auth.secret is required".into()));
                }
                Ok(())
            }
            AuthKind::MutualTls => Ok(()), // validated with TLS config cross-check
            AuthKind::Custom => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum AuthKind {
    None,
    UserPassword,
    Token,
    MutualTls,
    Custom, // e.g., Kerberos/IAM/etc.
}

/// Avoid printing secrets in Debug logs.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(**redacted**)")
    }
}

/// -------------------------
/// TLS / SSL
/// -------------------------

#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub enabled: bool,

    /// Verify server certificate & hostname.
    pub verify_peer: bool,

    /// Optional SNI override.
    pub server_name: Option<String>,

    /// Paths are intentionally generic; your transport can interpret them.
    pub ca_cert_path: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            verify_peer: true,
            server_name: None,
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

impl TlsConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.enabled {
            return Ok(());
        }
        if self.verify_peer && self.ca_cert_path.as_deref().unwrap_or("").is_empty() {
            // Allow system trust store in some stacks; keep it permissive:
            // return Err(...) if you want strict.
        }
        Ok(())
    }
}

/// -------------------------
/// Pooling
/// -------------------------

#[derive(Clone, Debug)]
pub struct PoolingConfig {
    pub enabled: bool,

    /// Max concurrent sessions / pooled connections.
    pub max_size: usize,

    /// Optional: keep at least this many warm.
    pub min_size: usize,

    /// Recycle a connection if it sits idle longer than this.
    pub max_idle: Option<Duration>,

    /// Max lifetime for a connection (even if busy).
    pub max_lifetime: Option<Duration>,

    /// If checkout blocks longer than this, fail.
    pub checkout_timeout: Duration,
}

impl Default for PoolingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_size: 10,
            min_size: 0,
            max_idle: Some(Duration::from_secs(300)),
            max_lifetime: None,
            checkout_timeout: Duration::from_secs(5),
        }
    }
}

impl PoolingConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.enabled {
            return Ok(());
        }
        if self.max_size == 0 {
            return Err(ConfigError::Invalid("pooling.max_size must be > 0".into()));
        }
        if self.min_size > self.max_size {
            return Err(ConfigError::Invalid(
                "pooling.min_size must be <= pooling.max_size".into(),
            ));
        }
        Ok(())
    }
}

/// -------------------------
/// Timeouts & retries (policy only)
/// -------------------------

#[derive(Clone, Debug)]
pub struct TimeoutConfig {
    pub connect: Duration,
    pub handshake: Duration,

    /// Operation-level default (query/command/etc.). Protocol may override per call.
    pub request: Duration,

    /// How long we allow an idle connection to wait for a response on recv.
    pub read: Duration,

    /// How long we allow sending bytes/frames.
    pub write: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            handshake: Duration::from_secs(5),
            request: Duration::from_secs(30),
            read: Duration::from_secs(30),
            write: Duration::from_secs(30),
        }
    }
}

impl TimeoutConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.connect.is_zero() {
            return Err(ConfigError::Invalid("timeouts.connect must be > 0".into()));
        }
        if self.request.is_zero() {
            return Err(ConfigError::Invalid("timeouts.request must be > 0".into()));
        }
        Ok(())
    }
}

/// -------------------------
/// Session policy (what to reset, what to keep)
/// -------------------------

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// If true: when returning to pool, ensure any open transaction is rolled back.
    pub rollback_on_return: bool,

    /// If true: protocol should do a "RESET SESSION"/equivalent if supported.
    pub full_reset_on_return: bool,

    /// Optional session vars (e.g., schema/search_path/role) applied on checkout.
    pub session_vars: BTreeMap<String, String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            rollback_on_return: true,
            full_reset_on_return: false,
            session_vars: BTreeMap::new(),
        }
    }
}

impl SessionConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}

/// -------------------------
/// Observability
/// -------------------------

#[derive(Clone, Debug)]
pub struct ObservabilityConfig {
    pub enable_metrics: bool,
    pub enable_tracing: bool,

    /// If enabled, you should redact secrets in logs.
    pub log_requests: bool,
    pub log_responses: bool,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            enable_metrics: true,
            enable_tracing: true,
            log_requests: false,
            log_responses: false,
        }
    }
}

impl ObservabilityConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        Ok(())
    }
}

/// -------------------------
/// Errors
/// -------------------------

#[derive(Debug)]
pub enum ConfigError {
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Invalid(s) => write!(f, "invalid config: {s}"),
        }
    }
}

impl std::error::Error for ConfigError {}
