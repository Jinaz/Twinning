use anyhow::{anyhow, Context, Result};
use std::{
    fmt::Debug,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;

/// Minimal capabilities a connection must provide for session management.
/// You can expand this with protocol-specific calls (e.g., reset, ping, auth refresh).
#[async_trait::async_trait]
pub trait ManagedConnection: Send + Sync + Debug + 'static {
    /// Fast liveness check. Should be cheap (e.g., protocol ping, socket check).
    async fn is_healthy(&mut self) -> bool;

    /// Reset any session state before returning to pool (optional but recommended).
    /// Examples: rollback open tx, reset search_path, clear temp settings, etc.
    async fn reset(&mut self) -> Result<()>;

    /// Close underlying resources (socket/file handles).
    async fn close(&mut self) -> Result<()>;
}

/// Factory for creating new connections.
/// Your DB-specific connector implements this by dialing the server, negotiating protocol, auth, etc.
#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync + 'static {
    type Conn: ManagedConnection;

    async fn connect(&self) -> Result<Self::Conn>;
}

/// Pool/session configuration.
#[derive(Clone, Debug)]
pub struct ConnectorConfig {
    /// Max pooled connections. Also bounds concurrent sessions.
    pub max_size: usize,

    /// Minimum number of connections to keep prewarmed (optional; best-effort).
    pub min_size: usize,

    /// If a checkout cannot obtain a connection before this duration, error.
    pub checkout_timeout: Duration,

    /// Drop (recreate) connections that have been idle longer than this.
    pub max_idle: Option<Duration>,

    /// Periodic background prewarm interval (if min_size > 0).
    pub prewarm_interval: Duration,
}

/// Internal pooled entry.
#[derive(Debug)]
struct Pooled<C: ManagedConnection> {
    conn: C,
    last_used: Instant,
    id: u64,
}

/// Generic connector that manages pooled connections and hands out sessions.
#[derive(Debug)]
pub struct Connector<F: ConnectionFactory> {
    factory: Arc<F>,
    cfg: ConnectorConfig,

    // Available connections.
    idle: Arc<Mutex<Vec<Pooled<F::Conn>>>>,

    // Capacity permits (max_size).
    permits: Arc<Semaphore>,

    // Unique connection ids.
    next_id: AtomicU64,
}

/// A checked-out session. On Drop, it resets and returns connection to the pool.
/// If reset fails, the connection is closed instead of being returned.
#[derive(Debug)]
pub struct Session<F: ConnectionFactory> {
    pooled: Option<Pooled<F::Conn>>,
    owner: Arc<Connector<F>>,
    _permit: OwnedSemaphorePermit,

    // Example state you may want to track for reset logic
    in_tx: bool,
}

#[derive(Debug)]
struct SessionDropShim<F: ConnectionFactory> {
    pooled: Option<Pooled<F::Conn>>,
    owner: Arc<Connector<F>>,
}


impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            min_size: 0,
            checkout_timeout: Duration::from_secs(5),
            max_idle: Some(Duration::from_secs(300)),
            prewarm_interval: Duration::from_secs(10),
        }
    }
}
impl<F: ConnectionFactory> Connector<F> {
    pub fn new(factory: F, cfg: ConnectorConfig) -> Arc<Self> {
        let permits = Arc::new(Semaphore::new(cfg.max_size));
        Arc::new(Self {
            factory: Arc::new(factory),
            cfg,
            idle: Arc::new(Mutex::new(Vec::new())),
            permits,
            next_id: AtomicU64::new(1),
        })
    }

    /// Optional: start background prewarming to maintain min_size connections.
    /// You can ignore this if you prefer lazy connection creation.
    pub fn start_prewarm(self: &Arc<Self>) {
        if self.cfg.min_size == 0 {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                // best-effort: try to create until we reach min_size
                let current_idle = { me.idle.lock().await.len() };
                let target = me.cfg.min_size;
                if current_idle < target {
                    let to_add = target - current_idle;
                    for _ in 0..to_add {
                        // Don't block forever; skip on failure.
                        let _ = me.try_add_one().await;
                    }
                }
                tokio::time::sleep(me.cfg.prewarm_interval).await;
            }
        });
    }

    /// Check out a session. Will block until available or timeout.
    pub async fn session(self: &Arc<Self>) -> Result<Session<F>> {
        // Bound concurrent sessions to max_size.
        let permit_fut = self.permits.clone().acquire_owned();
        let permit = timeout(self.cfg.checkout_timeout, permit_fut)
            .await
            .context("checkout timed out waiting for pool capacity")?
            .context("semaphore closed")?;

        // Prefer reusing an idle connection.
        if let Some(mut pooled) = self.take_idle().await {
            if self.should_recycle(&pooled) {
                pooled.conn.close().await.ok();
            } else if pooled.conn.is_healthy().await {
                return Ok(Session {
                    pooled: Some(pooled),
                    owner: Arc::clone(self),
                    _permit: permit,
                    in_tx: false,
                });
            } else {
                // unhealthy: close and replace
                pooled.conn.close().await.ok();
            }
        }

        // Create a new connection.
        let conn = self
            .factory
            .connect()
            .await
            .context("failed to create new connection")?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let pooled = Pooled {
            conn,
            last_used: Instant::now(),
            id,
        };

        Ok(Session {
            pooled: Some(pooled),
            owner: Arc::clone(self),
            _permit: permit,
            in_tx: false,
        })
    }

    async fn take_idle(&self) -> Option<Pooled<F::Conn>> {
        let mut idle = self.idle.lock().await;
        idle.pop()
    }

    fn should_recycle(&self, pooled: &Pooled<F::Conn>) -> bool {
        match self.cfg.max_idle {
            Some(max_idle) => pooled.last_used.elapsed() > max_idle,
            None => false,
        }
    }

    async fn return_idle(&self, mut pooled: Pooled<F::Conn>) {
        pooled.last_used = Instant::now();
        let mut idle = self.idle.lock().await;
        idle.push(pooled);
    }

    async fn try_add_one(&self) -> Result<()> {
        // Only create if we can take a permit without waiting.
        let permit = match self.permits.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return Ok(()), // at capacity
        };

        // Create connection; if fail, permit drops here.
        let conn = self.factory.connect().await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let pooled = Pooled {
            conn,
            last_used: Instant::now(),
            id,
        };
        self.return_idle(pooled).await;

        // Important: keep capacity by NOT dropping permit.
        // But we do want min_size connections to count toward max_size capacity,
        // so we *must* store the permit somewhere if we prewarm.
        //
        // Simpler: do NOT prewarm permits; prewarm only builds idle list without consuming capacity.
        // That would exceed max_size. So instead, we avoid this complexity:
        // We drop the permit here and let max_size be enforced only on checkout.
        //
        // If you want strict "open connections <= max_size", implement a separate counter
        // and consume a permanent permit for every created connection. Kept minimal here.
        drop(permit);

        Ok(())
    }
}

impl<F: ConnectionFactory> Session<F> {
    /// Access the underlying connection to run protocol-specific operations.
    pub fn conn_mut(&mut self) -> Result<&mut F::Conn> {
        self.pooled
            .as_mut()
            .map(|p| &mut p.conn)
            .ok_or_else(|| anyhow!("session already released"))
    }

    /// Mark transaction state for your own protocol integration.
    pub fn set_in_tx(&mut self, v: bool) {
        self.in_tx = v;
    }

    /// Explicitly release early (optional). Drop does this automatically.
    pub async fn release(mut self) -> Result<()> {
        self.release_inner().await
    }

    async fn release_inner(&mut self) -> Result<()> {
        let Some(mut pooled) = self.pooled.take() else {
            return Ok(());
        };

        // Example policy: if session thinks it's in a transaction, we rely on reset() to rollback.
        // Your DB-specific impl should do the right thing.
        let reset_ok = pooled.conn.reset().await.is_ok() && pooled.conn.is_healthy().await;

        if reset_ok && !self.owner.should_recycle(&pooled) {
            self.owner.return_idle(pooled).await;
            Ok(())
        } else {
            pooled.conn.close().await.ok();
            Ok(())
        }
    }
}

impl<F: ConnectionFactory> Drop for Session<F> {
    fn drop(&mut self) {
        // We can't .await in Drop; spawn a task to return/close.
        // This requires tokio runtime.
        if self.pooled.is_some() {
            let mut me = SessionDropShim {
                pooled: self.pooled.take(),
                owner: Arc::clone(&self.owner),
            };
            tokio::spawn(async move {
                if let Some(mut pooled) = me.pooled.take() {
                    let reset_ok = pooled.conn.reset().await.is_ok() && pooled.conn.is_healthy().await;
                    if reset_ok && !me.owner.should_recycle(&pooled) {
                        me.owner.return_idle(pooled).await;
                    } else {
                        pooled.conn.close().await.ok();
                    }
                }
            });
        }
    }
}
