// ... existing imports ...
use postgresdb::models::postgrestypes::DbMetadata;
use postgresdb::models::properties::position::{Position, PositionProperty};
use postgresdb::models::properties::speed::{Speed, SpeedProperty};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let pg_url = std::env::var("POSTGRES_URL").unwrap_or_else(|_| {
    let user = std::env::var("USER").unwrap_or_else(|_| "postgres".to_string());
    format!("postgres://{}@localhost:5432/postgres", user)
});


    let manager = std::sync::Arc::new(postgresdb::databasemanager::DatabaseManager::new());

    let client = postgresdb::postgresclient::PostgresClient::connect(&pg_url).await?;
    let pgdb = std::sync::Arc::new(postgresdb::postgresimpl::PostgresDatabase::new("pg-main", client).await);

    manager.add_db(pgdb.clone()).await?;

    println!("Database created");

    let table = "dt_sensor";
    pgdb.create_table(
        table,
        postgresdb::postgresimpl::PostgresDatabase::default_timeseries_columns(
            postgresdb::models::postgrestypes::PgDataType::Float8,
        ),
    )
    .await?;

    pgdb.refresh_schema_from_db().await?;
    let meta = pgdb.metadata_snapshot().await?;
    println!("{}", serde_json::to_string_pretty(&meta)?);


    // Register DT identifiers -> where they live
    pgdb.register_binding("dt.sensor.temperature", table, Some("payload")).await?;
    pgdb.register_binding("dt.sensor.temperature.timestamp", table, Some("ts")).await?;
    
    // Print metadata snapshot
    let meta: DbMetadata = pgdb.metadata_snapshot().await?;
    println!("--- METADATA SNAPSHOT ---\n{}", serde_json::to_string_pretty(&meta)?);

    pgdb.ensure_position_schema().await?;
    pgdb.ensure_speed_schema().await?;

    pgdb.try_set_position("car-1", Position::new(1.0, 2.0, 3.0))?;
    pgdb.try_set_speed("car-1", Speed::new(42.0))?;


// inside some high-frequency DT loop:
for tick in 0..10_000u64 {
    let payload = (tick as f64) * 0.01;

    // fire-and-forget; never await
    if let Err(e) = pgdb.try_submit_insert_row(
        "dt_sensor",
        vec![
            ("id".into(), postgresdb::models::transformation::SqlParam::uuid(uuid::Uuid::new_v4())),
            ("payload".into(), postgresdb::models::transformation::SqlParam::f64(payload)),
        ],
    ) {
        if matches!(e, postgresdb::postgresimpl::DbError::QueueFull) {
            // RT decision: drop this tick's sample (or count it)
            // no await, no panic
        }
    }

    // ... rest of real-time loop continues ...
}

    // cleanup (optional)
    //pgdb.drop_table(table).await?;
    //manager.remove_db("pg-main").await;

    Ok(())
}
