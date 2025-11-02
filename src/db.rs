use crate::entur_data::{append_data, VehicleJourneyAppend};
use duckdb::Connection;
use tracing::info;

pub fn prepare_db(db_url: &Option<String>, parquet_root: &str) -> anyhow::Result<Connection> {
    info!("Prepare database {:?}", db_url);

    let db = match db_url {
        None => Connection::open_in_memory(),
        Some(f) => Connection::open(f.as_str()),
    }?;

    let schema: &str = include_str!("schema.sql");

    db.execute_batch(schema)?;

    let quays = format!("{}/quays.parquet", parquet_root.trim_end_matches('/'));
    let stops = format!("{}/stops.parquet", parquet_root.trim_end_matches('/'));
    info!("Create quays={quays} and stops={stops} in DuckDB");
    db.execute(
        "create or replace table quays as from read_parquet($1);",
        [quays.as_str()],
    )?;
    db.execute(
        "create or replace table stops as from read_parquet($1);",
        [stops.as_str()],
    )?;

    Ok(db)
}

pub fn replace_data(
    db: &mut Connection,
    data: impl Iterator<Item = VehicleJourneyAppend>,
) -> anyhow::Result<()> {
    let tx = db.transaction()?;
    append_data(
        data,
        tx.appender("vehicle_journey")?,
        tx.appender("estimated_call")?,
        tx.appender("recorded_call")?,
    )?;
    // TODO: Get rid of old versions of the journeys + calls
    tx.commit()?;
    Ok(())
}
