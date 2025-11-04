use crate::entur_data::{optional_timestamptz, timestamptz};
use chrono::{DateTime, FixedOffset};
use duckdb::Connection;
use serde::Serialize;

#[derive(Serialize)]
pub struct JourneyDelay {
    vehicle_journey_id: String,
    line_ref: String,
    last_stop_name: String,
    aimed_last_stop_time: DateTime<FixedOffset>,
    actual_last_stop_time: DateTime<FixedOffset>,
    recorded_delay_seconds: i32,
    next_stop_name: Option<String>,
    aimed_next_stop_time: Option<DateTime<FixedOffset>>,
}

pub fn journey_delays(
    stop_name_filter: &str,
    conn: &Connection,
) -> duckdb::Result<Vec<JourneyDelay>> {
    let q = include_str!("by_stop_name.sql");
    conn.prepare(q)?
        .query_map([stop_name_filter], |row| {
            Ok(JourneyDelay {
                vehicle_journey_id: row.get(0)?,
                line_ref: row.get(1)?,
                last_stop_name: row.get(2)?,
                aimed_last_stop_time: timestamptz(row.get(3)?),
                actual_last_stop_time: timestamptz(row.get(4)?),
                recorded_delay_seconds: row.get(5)?,
                next_stop_name: row.get(6)?,
                aimed_next_stop_time: optional_timestamptz(row.get(7)?),
            })
        })?
        .collect()
}
