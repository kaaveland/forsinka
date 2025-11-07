use crate::entur_data::{VehicleJourneyAppend, append_data};
use duckdb::Connection;
use ordered_float::OrderedFloat;
use std::time::Instant;
use tracing::{info, instrument};

const STOP_DATA: &str = "
create or replace table stopdata as
from stops s
  join quays q on q.stopPlaceRef = s.id
select
  s.name as name,
  q.id as stop_point_ref,
  coalesce(q.location_latitude, s.location_latitude) as lat,
  coalesce(q.location_longitude, s.location_longitude) as lon
";

pub struct StopRow {
    pub name: String,
    pub stop_point_ref: String,
    pub lat: Option<OrderedFloat<f32>>,
    pub lon: Option<OrderedFloat<f32>>,
}

pub fn read_stops(db: &Connection) -> duckdb::Result<Vec<StopRow>> {
    db.prepare("from stopdata select name, stop_point_ref, lat, lon where name is not null")?
        .query_map([], |row| {
            Ok(StopRow {
                name: row.get(0)?,
                stop_point_ref: row.get(1)?,
                lat: row.get::<_, Option<f32>>(2)?.map(OrderedFloat),
                lon: row.get::<_, Option<f32>>(3)?.map(OrderedFloat),
            })
        })?
        .collect()
}

pub fn prepare_db(
    db_url: &Option<String>,
    parquet_root: &str,
    threads: u8,
    memory_gb: u8,
) -> anyhow::Result<Connection> {
    info!("Prepare database {:?}", db_url);

    let db = match db_url {
        None => Connection::open_in_memory(),
        Some(f) => Connection::open(f.as_str()),
    }?;

    db.execute_batch(
        format!(
            "set memory_limit = '{}GB'; set threads = {}",
            memory_gb, threads
        )
        .as_str(),
    )?;

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
    db.execute_batch(STOP_DATA)?;

    Ok(db)
}

const WITH_CURRENT: &str = "
with current as (
  from vehicle_journey
  select distinct on(vehicle_journey_id) vehicle_journey_id, version, recorded_at_time
  order by version desc, recorded_at_time desc
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

    tx.execute_batch(
        "delete from vehicle_journey where finished and age(recorded_at_time) > interval 12 hours;",
    )?;

    tx.execute_batch(
            format!(
                "create or replace table estimated_call as {WITH_CURRENT}
                  from estimated_call join current using(vehicle_journey_id, version, recorded_at_time);
                 create index estimated_call_idx on estimated_call(vehicle_journey_id);"
            )
            .as_str(),
        )?;

    tx.execute_batch(
            format!(
                "create or replace table recorded_call as {WITH_CURRENT}
                  from recorded_call join current using(vehicle_journey_id, version, recorded_at_time);
                 create index recorded_call_idx on recorded_call(vehicle_journey_id);"
            )
            .as_str(),
        )?;

    tx.execute_batch(
            format!(
                "create or replace table vehicle_journey as {WITH_CURRENT}
                   from vehicle_journey join current using(vehicle_journey_id, version, recorded_at_time);
                 create index vehicle_journey_idx on recorded_call(vehicle_journey_id);"
            )
                .as_str(),
        )?;

    tx.commit()?;
    tracing::Span::current().record("duration_ms", start.elapsed().as_millis());
    db.execute_batch("checkpoint; analyze")?;
    Ok(())
}
