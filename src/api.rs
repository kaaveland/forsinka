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

const CURRENT_TRAIN: &str = "
with next_stop as (
  from estimated_call join stopdata s using (stop_point_ref)
  select distinct on (vehicle_journey_id)
    s.name next_stop, aimed_arrival_time next_stop_time, vehicle_journey_id, data_source
  where data_source in ('VYG', 'BNR', 'SJN', 'FLY', 'FLT')
  order by aimed_arrival_time asc
), prev_stop as (
  from recorded_call join stopdata s using (stop_point_ref)
  select distinct on (vehicle_journey_id)
   s.name stop_name,
   coalesce(aimed_arrival_time, aimed_departure_time) aimed_time,
   coalesce(actual_arrival_time, actual_departure_time) actual_time,
  actual_time - aimed_time as delay,
   (extract (epoch from delay)) :: int4 as delay_seconds,
   vehicle_journey_id,
   data_source
  where data_source in ('VYG', 'BNR', 'SJN', 'FLY', 'FLT')
  order by aimed_time desc
)
from vehicle_journey vj join prev_stop rc using(vehicle_journey_id, data_source)
  left join next_stop ns using(vehicle_journey_id, data_source)
select
  vj.vehicle_journey_id,
  vj.line_ref,
  vj.cancellation,
  vj.data_source,
  rc.stop_name,
  ns.next_stop next_stop_name,
  rc.aimed_time,
  rc.actual_time,
  rc.delay_seconds,
  ns.next_stop_time
where (vj.started and not vj.finished) and vj.data_source in ('VYG', 'BNR', 'SJN', 'FLY', 'FLT')
order by vj.vehicle_journey_id, aimed_time desc
";

#[derive(Serialize)]
pub struct TrainJourney {
    vehicle_journey_id: String,
    line_ref: String,
    cancellation: bool,
    data_source: String,
    stop_name: String,
    next_stop_name: Option<String>,
    aimed_time: DateTime<FixedOffset>,
    actual_time: DateTime<FixedOffset>,
    delay_seconds: i32,
    next_stop_time: Option<DateTime<FixedOffset>>,
}

pub fn train_journeys(conn: &Connection) -> duckdb::Result<Vec<TrainJourney>> {
    conn.prepare(CURRENT_TRAIN)?
        .query_map([], |row| {
            Ok(TrainJourney {
                vehicle_journey_id: row.get(0)?,
                line_ref: row.get(1)?,
                cancellation: row.get(2)?,
                data_source: row.get(3)?,
                stop_name: row.get(4)?,
                next_stop_name: row.get(5)?,
                aimed_time: timestamptz(row.get(6)?),
                actual_time: timestamptz(row.get(7)?),
                delay_seconds: row.get(8)?,
                next_stop_time: optional_timestamptz(row.get(9)?),
            })
        })?
        .collect()
}
