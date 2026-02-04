# Service Layer - Digital Twin System (Rust)

## Aufbau

Der Service Layer besteht aus drei Crates, die hierarchisch aufeinander aufbauen:

```
services/
  rtservices/           # RT-Kern: Traits, OS-RT, Timing, Properties
  nonrtservices/        # Non-RT: REST API, DB-Writer, Service-Trait
  servicemanagement/    # Orchestrierung: Manager, Registry, Dispatcher, Engine
```

---

## 1. rtservices

Stellt die gesamte Real-Time-Infrastruktur bereit.

### RtService-Trait (`traits.rs`)

Jeder RT-Service implementiert dieses Trait:

```rust
pub trait RtService: Send {
    fn name(&self) -> &str;
    fn config(&self) -> &RtConfig;
    fn init(&mut self) -> RtResult<()>  { Ok(()) }
    fn tick(&mut self, ctx: &RtContext<'_>) -> RtResult<()>;
    fn shutdown(&mut self) -> RtResult<()> { Ok(()) }
}
```

- `tick()` wird jeden Zyklus aufgerufen und bekommt einen `RtContext` mit Zugriff auf die atomaren Properties und den Event-Channel.
- `RtConfig` definiert `period` und `wcet` (Worst-Case Execution Time).

### Atomare Properties (`properties.rs`)

Lock-freier Zugriff auf geteilte Werte zwischen RT- und Non-RT-Schicht:

```rust
pub struct Properties {
    speed: AtomicU64,  // speichert f64 als Bits
}
```

- `get_speed()` / `set_speed()` verwenden `Ordering::Acquire/Release`.
- Kein Mutex, kein Lock -- sicher aus dem RT-Thread aufrufbar.

### Contexts (`context.rs`)

Zwei verschiedene Kontexte für die zwei Schichten:

| Kontext | Felder | Eigenschaften                            |
|---------|--------|------------------------------------------|
| `RtContext<'a>` | `&'a Properties`, `&'a Sender<EngineEvent>` | Borrow-basiert, nicht klonbar, RT-sicher |
| `NonRtContext` | `Arc<Properties>`, `Sender<EngineEvent>` | Klonbar, für async/Non-RT Nutzung        |

### EngineEvent (`engine_event.rs`)

Event-Typ für die Kommunikation zwischen Services und Engine:

```rust
pub enum EngineEvent {
    SetSpeed(f64),
    Custom { topic: String, payload: serde_json::Value },
}
```

Gesendet über `crossbeam_channel` (bounded, lock-free, RT-tauglich).

### RtGuard und Budget-Checking (`rt_guard.rs`)

Messung der Ausführungszeit pro Service pro Tick:

- `RtGuard` -- startet einen `Instant`-Timer.
- `RtServiceStats` -- ählt `calls`, `overruns` (u32), `last_elapsed`, `max_elapsed`, `disabled`.
- `RtCheckConfig` -- konfiguriert `disable_after_overruns` (Default: 3).
- `update_stats()` -- prüft nach jedem Tick ob das Budget überschritten wurde und deaktiviert den Service nach zu vielen Overruns.

### RtChecker - OS-Level RT (`rt_checker.rs`)

Verwaltet die OS-seitigen Real-Time-Einstellungen:

| Funktion | Beschreibung                                                | Voraussetzung |
|----------|-------------------------------------------------------------|---------------|
| `pin_to_cpu(core)` | CPU-Affinität via `sched_setaffinity`                       | Linux |
| `set_fifo_priority(prio)` | SCHED_FIFO Scheduler (Prio 1-99)                            | Linux, root |
| `lock_memory()` | `mlockall(MCL_CURRENT \| MCL_FUTURE)`                       | Linux, root/RLIMIT |
| `start_monitor()` | Startet Monitor-Thread mit `clock_nanosleep(TIMER_ABSTIME)` | Linux |
| `snapshot()` | Liefert `RtStats { samples, misses, max_late_ns }`          | - |
| `stop()` | Stoppt den Monitor-Thread                                   | - |

Auf Windows kompiliert alles, die RT-Funktionen geben `PlatformNotSupported` zurück.

### Beispiel-Services

- **PositionSyncService** (`services/position_sync.rs`) -- Synchronisiert Positionen über atomare Werte.
- **ServiceA** (`services/service_a.rs`) -- Demo-Service, liest Speed, sendet alle 50 Ticks ein `SetSpeed`-Event.

---

## 2. nonrtservices

Stellt die Non-Real-Time-Infrastruktur bereit (async, darf blocken).

### NonRtService-Trait (`traits.rs`)

```rust
#[async_trait]
pub trait NonRtService: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&mut self) -> ServiceResult<()>;
    async fn stop(&mut self) -> ServiceResult<()>;
    async fn health_check(&self) -> bool;
    fn status(&self) -> ServiceStatus;
    fn call(&self, req: Value, ctx: &NonRtContext) -> Result<Value, ServiceError>;
}
```

`ServiceError` umfasst: `StartFailed`, `StopFailed`, `NotRunning`, `AlreadyRunning`, `ServiceNotFound`, `ServiceFailure`, `InvalidRequest`, `Internal`.

### REST API (`rest_api.rs`)

Axum-basierter HTTP-Server mit folgenden Endpoints:

| Methode | Pfad | Beschreibung |
|---------|------|-------------|
| GET | `/health` | Health Check |
| GET | `/services` | Liste aller Services |
| GET | `/services/{name}/status` | Status eines Services |
| POST | `/services/{name}/start` | Service starten |
| POST | `/services/{name}/stop` | Service stoppen |
| POST | `/services/{name}` | Service aufrufen (call) |
| GET | `/properties/speed` | Aktuelle Geschwindigkeit lesen |
| PUT | `/properties/speed` | Geschwindigkeit setzen |
| POST | `/events` | Engine-Event injizieren |

Der `ApiState` hält optional `Arc<Properties>` und `Sender<EngineEvent>`, sodass die REST API direkt auf die atomaren Werte und den Event-Channel zugreifen kann.

### DbWriterService (`services/db_writer.rs`)

Actor-basierter Datenbank-Writer (Platzhalter für PostgreSQL-Anbindung).

---

## 3. servicemanagement

Orchestriert RT- und Non-RT-Services.

### ServiceManager (`manager.rs`)

Zentrale Komponente, die alles zusammenhält:

```rust
let mut manager = ServiceManager::with_config(config);

// RT-Services mit Budget registrieren
manager.register_rt(Box::new(service_a), RtRegistration {
    budget: Duration::from_micros(500),
});

// Non-RT-Services registrieren
manager.register_nonrt(Box::new(rest_api)).await;

// Alles starten
manager.start_all().await?;

// RT-Tick ausführen (aus dediziertem Thread)
manager.tick_rt();
```

**Kernfunktionen:**
- `tick_rt()` -- Iteriert alle RT-Services, misst per `RtGuard` die Zeit, prüft Budget, deaktiviert bei Overruns.
- `rt_stats_snapshot()` -- Liefert aktuelle Statistiken pro RT-Service.
- `nonrt_dispatcher(capacity)` -- Erzeugt einen `NonRtDispatcher` mit eigenem Worker-Thread.
- `start_all()` / `stop_all()` -- Lifecycle-Management für alle Services.
- `props()` -- Zugriff auf die geteilten atomaren `Properties`.
- `event_tx()` / `event_rx()` -- Event-Channel Zugriff.

### NonRtDispatcher (`dispatcher.rs`)

Dedizierter Worker-Thread für Non-RT-Service-Aufrufe:

```rust
let dispatcher = manager.nonrt_dispatcher(64);
let result = dispatcher.call("db_writer", json!({"query": "..."}));
```

Verwendet `crossbeam_channel` (bounded) für die Kommunikation und einen eigenen Tokio-Runtime auf dem Worker-Thread.

### ServiceRegistry (`registry.rs`)

Zentrale Registry für Service-Lookup nach Name. Speichert RT- und Non-RT-Services getrennt.

### ServiceHierarchy (`hierarchy.rs`)

Baumstruktur für die hierarchische Organisation von Services (Container: `realtime`, `nonrealtime`, `system`).

### SampleEngine (`sample_engine.rs`)

Minimale Engine-Implementierung zum Testen:

```rust
let mut engine = SampleEngine::new();
engine.manager_mut().register_rt(service, registration);
engine.run_for(Duration::from_secs(1), Duration::from_millis(10));
```

- `run_for(run_time, period)` -- Führt den RT-Loop für eine gegebene Daür aus.
- `process_events()` -- Verarbeitet Events aus dem Channel (z.B. `SetSpeed`).

---

## Architektur-Überblick

```
                    REST API (Axum)
                        |
                   NonRtContext
                   (Arc<Properties>, Sender)
                        |
    +-------------------+-------------------+
    |                                       |
NonRtDispatcher                      ServiceManager
(Worker-Thread)                      (Orchestrierung)
    |                                       |
NonRtService                          tick_rt()
(async, darf blocken)                       |
                                      RtContext<'a>
                                      (&Properties, &Sender)
                                            |
                                    +-------+-------+
                                    |               |
                               ServiceA      PositionSync
                               (RT-Service)   (RT-Service)

    Properties (AtomicU64) <--- lock-free ---> Event Channel (crossbeam)
```

---

## Tests ausführen

```bash
cargo test -p rtservices -p nonrtservices -p servicemanagement
```

Aktuell 18 Tests, alle bestanden:
- `rtservices`: 9 Tests (Traits, Properties, Guard, Checker, Services)
- `nonrtservices`: 3 Tests (Traits, REST API State, DbWriter)
- `servicemanagement`: 6 Tests (Manager, Registry, Hierarchy, SampleEngine)

---