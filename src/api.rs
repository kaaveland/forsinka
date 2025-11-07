use crate::entur_data::{optional_timestamptz, timestamptz};
use askama::Template;
use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::Europe::Oslo;
use duckdb::Connection;
use serde::Serialize;

#[derive(Serialize)]
pub struct JourneyDelay {
    pub vehicle_journey_id: String,
    pub line_ref: String,
    pub last_stop_name: String,
    pub aimed_last_stop_time: DateTime<FixedOffset>,
    pub actual_last_stop_time: DateTime<FixedOffset>,
    pub recorded_delay_seconds: i32,
    pub next_stop_name: Option<String>,
    pub aimed_next_stop_time: Option<DateTime<FixedOffset>>,
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
    actual_departure_time is null as departed,
    actual_time - aimed_time as delay,
    (extract (epoch from delay)) :: int4 as delay_seconds,
    vehicle_journey_id,
    data_source,
    recorded_at_time
  where data_source in ('VYG', 'BNR', 'SJN', 'FLY', 'FLT')
  order by aimed_time desc
), complete as (
  from vehicle_journey vj join prev_stop rc using(vehicle_journey_id, data_source)
    left join next_stop ns using(vehicle_journey_id, data_source)
  select distinct on(vj.vehicle_journey_id)
    vj.vehicle_journey_id,
    vj.line_ref,
    vj.cancellation,
    vj.data_source,
    rc.stop_name,
    ns.next_stop next_stop_name,
    rc.aimed_time,
    rc.actual_time,
    rc.delay_seconds,
    ns.next_stop_time,
    rc.departed,
    -- The train might be stuck if the current timestamp is larger than the previous arrival time
    -- plus the planned travel time to the next stop plus the known delay plus a 5 minute safety margin.
    -- Stated differently: if it gained another 5 minutes of delay between rc.stop_name and ns.next_stop
    coalesce(
      rc.aimed_time +
      (ns.next_stop_time - rc.aimed_time) -- planned travel time
      + greatest(interval '0 minutes', rc.delay) + interval '5 minutes' -- 5 minute extra delay on this leg
    < now(), false) as possibly_stuck,
  where (vj.started and not vj.finished) and vj.data_source in ('VYG', 'BNR', 'SJN', 'FLY', 'FLT')
  order by vj.recorded_at_time, rc.recorded_at_time)
from complete select *
order by possibly_stuck desc, delay_seconds desc, actual_time desc
";

#[derive(Serialize)]
pub struct TrainJourney {
    pub vehicle_journey_id: String,
    pub line_ref: String,
    pub cancellation: bool,
    pub data_source: String,
    pub stop_name: String,
    pub next_stop_name: Option<String>,
    pub aimed_time: DateTime<FixedOffset>,
    pub actual_time: DateTime<FixedOffset>,
    pub delay_seconds: i32,
    pub next_stop_time: Option<DateTime<FixedOffset>>,
    pub departed: bool,
    pub possibly_stuck: bool,
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
                departed: row.get(10)?,
                possibly_stuck: row.get(11)?,
            })
        })?
        .collect()
}

#[derive(Template)]
#[template(path = "trains.html")]
pub struct TrainsPage {
    pub trains: Vec<TrainJourney>,
    pub timestamp: String,
    pub delayed_count: usize,
    pub stuck_count: usize,
    pub assets_path: String,
}

impl TrainsPage {
    pub fn new(trains: Vec<TrainJourney>, assets_path: String) -> Self {
        let delayed_count = trains.iter().filter(|t| t.delay_seconds > 60).count();
        let stuck_count = trains.iter().filter(|t| t.possibly_stuck).count();
        let now_oslo = Utc::now().with_timezone(&Oslo);
        let timestamp = now_oslo.format("%Y-%m-%d %H:%M:%S").to_string();

        Self {
            trains,
            timestamp,
            delayed_count,
            stuck_count,
            assets_path,
        }
    }
}

// Askama template filters
mod filters {
    use chrono::{DateTime, FixedOffset};
    use chrono_tz::Europe::Oslo;

    pub fn format_time(dt: &DateTime<FixedOffset>) -> ::askama::Result<String> {
        let oslo_time = dt.with_timezone(&Oslo);
        Ok(oslo_time.format("%H:%M").to_string())
    }

    pub fn format_delay(seconds: &i32) -> ::askama::Result<String> {
        let minutes = seconds / 60;
        Ok(format!("{} min", minutes))
    }
}
