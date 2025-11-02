use crate::entur_data::{append_data, VehicleJourneyAppend};
use duckdb::Connection;
use std::time::Instant;
use tracing::{info, instrument};

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

const WITH_CURRENT: &str = "
with current as (
  from vehicle_journey
  select distinct on(vehicle_journey_id) vehicle_journey_id, recorded_at_time
  order by recorded_at_time desc
)
";

#[instrument(name = "replace_data", skip(db, data), fields(duration_ms = tracing::field::Empty))]
pub fn replace_data(
    db: &mut Connection,
    data: impl Iterator<Item = VehicleJourneyAppend>,
) -> anyhow::Result<()> {
    let start = Instant::now();
    let tx = db.transaction()?;
    append_data(
        data,
        tx.appender("vehicle_journey")?,
        tx.appender("estimated_call")?,
        tx.appender("recorded_call")?,
    )?;

    // TODO: Check this logic / timestamps - requires diving into the data. We may need more IDs to actually do this successfully.
    tx.execute_batch(
        format!(
            "create or replace table estimated_call as {WITH_CURRENT} from estimated_call join current using(vehicle_journey_id, recorded_at_time);"
        )
        .as_str(),
    )?;

    tx.execute_batch(
        format!(
            "create or replace table recorded_call as {WITH_CURRENT} from recorded_call join current using(vehicle_journey_id, recorded_at_time);"
        )
        .as_str(),
    )?;

    tx.execute_batch(
        format!(
            "create or replace table vehicle_journey as {WITH_CURRENT} from vehicle_journey join current using(vehicle_journey_id, recorded_at_time);"
        )
            .as_str(),
    )?;

    tx.commit()?;
    tracing::Span::current().record("duration_ms", start.elapsed().as_millis());
    Ok(())
}
