use crate::entur_siriformat::{EstimatedVehicleJourney, SiriETResponse};
use crate::membased::{Journeys, Stops};
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use duckdb::{Appender, Row};
use reqwest::Client;
use std::fs;
use tracing::{Level, event, info, instrument, span};

pub const ENTUR_API_URL: &str = "https://api.entur.io/realtime/v1/rest/et";

pub struct Config {
    requestor_id: String,
    api_url: String,
    client: Client,
    static_data: Option<String>,
}

impl Config {
    pub fn new(
        requestor_id: String,
        api_url: String,
        client: Client,
        static_data: Option<String>,
    ) -> Self {
        Self {
            requestor_id,
            api_url,
            client,
            static_data,
        }
    }
}

#[instrument(name = "fetch_siri", skip(config))]
async fn fetch_siri(config: &Config) -> anyhow::Result<SiriETResponse> {
    let url = config.api_url.as_str();
    let requestor_id = config.requestor_id.as_str();
    info!("Poll {url} with requestorId={requestor_id}");
    Ok(config
        .client
        .get(url)
        // TODO: We're getting the entire dataset each time for some reason?
        // Might not be a problem since we're pretty fast anyway.
        .query(&[("requestorId", requestor_id)])
        .header("Accept", "application/json")
        .send()
        .await?
        .json()
        .await?)
}

pub async fn fetch_data(config: &Config) -> anyhow::Result<SiriETResponse> {
    if let Some(path) = &config.static_data {
        let content = fs::read(path)?;
        Ok(serde_json::from_slice(&content)?)
    } else {
        fetch_siri(config).await
    }
}

pub async fn fetch_journeys<'a>(config: &Config, stops: &'a Stops) -> anyhow::Result<Journeys<'a>> {
    let data = fetch_data(config).await?;
    Ok(Journeys::new(
        stops,
        data.siri
            .service_delivery
            .estimated_timetable_delivery
            .into_iter()
            .flat_map(|et| {
                et.estimated_journey_version_frame
                    .into_iter()
                    .flat_map(|f| f.estimated_vehicle_journey.into_iter())
            }),
    ))
}

pub fn append_data(
    data: impl Iterator<Item = VehicleJourneyAppend>,
    mut journey_appender: Appender,
    mut estimated_calls_appender: Appender,
    mut recorded_calls_appender: Appender,
) -> duckdb::Result<()> {
    let _span = span!(Level::INFO, "append_data").entered();
    let mut journeys: usize = 0;
    let mut estimated_calls = 0;
    let mut recorded_calls = 0;

    for append in data {
        let VehicleJourneyAppend {
            vehicle_journey_row,
            estimated_call_rows,
            recorded_call_rows,
        } = append;
        vehicle_journey_row.append_with(&mut journey_appender)?;
        journeys += 1;
        estimated_calls += estimated_call_rows.len();
        for estimated_call_row in estimated_call_rows {
            estimated_call_row.append_with(&mut estimated_calls_appender)?;
        }
        recorded_calls += recorded_call_rows.len();
        for recorded_call_row in recorded_call_rows {
            recorded_call_row.append_with(&mut recorded_calls_appender)?;
        }
    }
    event!(
        Level::INFO,
        journeys = journeys,
        estimated_calls = estimated_calls,
        recorded_calls = recorded_calls
    );

    Ok(())
}

pub trait RowType {
    /// Maps 1-1 with a DuckDB-table and can be inserted with the Appender API for greater throughput.
    /// NB! It is important to match column positions correctly for this to work.
    fn append_with(self, appender: &mut Appender) -> duckdb::Result<()>;
    #[allow(unused)]
    fn map_from(row: &duckdb::Row) -> duckdb::Result<Self>
    where
        Self: Sized;
}

/// Maps 1-1 with DuckDB vehicle_journey table, when using the DuckDB appender API column positions must match!
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleJourneyRow {
    pub id: String,
    pub version: u32,
    pub data_source: String,
    pub recorded_at_time: DateTime<FixedOffset>,
    pub cancellation: bool,
    pub extra_journey: bool,
    pub line_ref: String,
    pub direction_ref: String,
    pub destination_ref: Option<String>,
    pub origin_ref: Option<String>,
    pub started: bool,
    pub finished: bool,
}

pub fn timestamptz(micros: i64) -> DateTime<FixedOffset> {
    // TODO: Preserve original timezone
    Utc.timestamp_micros(micros)
        .single()
        .expect("Invalid epoch timestamp")
        .into()
}

pub fn optional_timestamptz(col: Option<i64>) -> Option<DateTime<FixedOffset>> {
    col.map(timestamptz)
}

impl RowType for VehicleJourneyRow {
    fn append_with(self, appender: &mut Appender) -> duckdb::Result<()> {
        appender.append_row(duckdb::params![
            self.id,
            self.version,
            self.data_source,
            self.recorded_at_time,
            self.cancellation,
            self.extra_journey,
            self.line_ref,
            self.direction_ref,
            self.destination_ref,
            self.origin_ref,
            self.started,
            self.finished
        ])
    }

    fn map_from(row: &Row) -> duckdb::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            id: row.get(0)?,
            version: row.get(1)?,
            data_source: row.get(2)?,
            recorded_at_time: timestamptz(row.get(3)?),
            cancellation: row.get(4)?,
            extra_journey: row.get(5)?,
            line_ref: row.get(6)?,
            direction_ref: row.get(7)?,
            destination_ref: row.get(8)?,
            origin_ref: row.get(9)?,
            started: row.get(10)?,
            finished: row.get(11)?,
        })
    }
}

/// Maps 1-1 with DuckDB estimated_call table, when using the DuckDB appender API column positions must match!
#[derive(Clone, Debug, PartialEq)]
pub struct EstimatedCallRow {
    pub vehicle_journey_id: String,
    pub version: u32,
    pub data_source: String,
    pub recorded_at_time: DateTime<FixedOffset>,
    pub order: u16,
    pub aimed_arrival_time: Option<DateTime<FixedOffset>>,
    pub aimed_departure_time: Option<DateTime<FixedOffset>>,
    pub expected_arrival_time: Option<DateTime<FixedOffset>>,
    pub expected_departure_time: Option<DateTime<FixedOffset>>,
    pub cancellation: bool,
    pub stop_point_ref: String,
}

impl RowType for EstimatedCallRow {
    fn append_with(self, appender: &mut Appender) -> duckdb::Result<()> {
        appender.append_row(duckdb::params![
            self.vehicle_journey_id,
            self.version,
            self.data_source,
            self.recorded_at_time,
            self.order,
            self.aimed_arrival_time,
            self.aimed_departure_time,
            self.expected_arrival_time,
            self.expected_departure_time,
            self.cancellation,
            self.stop_point_ref
        ])
    }

    fn map_from(row: &Row) -> duckdb::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            vehicle_journey_id: row.get(0)?,
            version: row.get(1)?,
            data_source: row.get(2)?,
            recorded_at_time: timestamptz(row.get(3)?),
            order: row.get(4)?,
            aimed_arrival_time: optional_timestamptz(row.get(5)?),
            aimed_departure_time: optional_timestamptz(row.get(6)?),
            expected_arrival_time: optional_timestamptz(row.get(7)?),
            expected_departure_time: optional_timestamptz(row.get(8)?),
            cancellation: row.get(9)?,
            stop_point_ref: row.get(10)?,
        })
    }
}

/// Maps 1-1 with DuckDB recorded_call table, when using the DuckDB appender API column positions must match!
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedCallRow {
    pub vehicle_journey_id: String,
    pub version: u32,
    pub data_source: String,
    pub recorded_at_time: DateTime<FixedOffset>,
    pub order: u16,
    pub aimed_arrival_time: Option<DateTime<FixedOffset>>,
    pub aimed_departure_time: Option<DateTime<FixedOffset>>,
    pub actual_arrival_time: Option<DateTime<FixedOffset>>,
    pub actual_departure_time: Option<DateTime<FixedOffset>>,
    pub cancellation: bool,
    pub stop_point_ref: String,
}

impl RowType for RecordedCallRow {
    fn append_with(self, appender: &mut Appender) -> duckdb::Result<()> {
        appender.append_row(duckdb::params![
            self.vehicle_journey_id,
            self.version,
            self.data_source,
            self.recorded_at_time,
            self.order,
            self.aimed_arrival_time,
            self.aimed_departure_time,
            self.actual_arrival_time,
            self.actual_departure_time,
            self.cancellation,
            self.stop_point_ref
        ])
    }

    fn map_from(row: &Row) -> duckdb::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            vehicle_journey_id: row.get(0)?,
            version: row.get(1)?,
            data_source: row.get(2)?,
            recorded_at_time: timestamptz(row.get(3)?),
            order: row.get(4)?,
            aimed_arrival_time: optional_timestamptz(row.get(5)?),
            aimed_departure_time: optional_timestamptz(row.get(6)?),
            actual_arrival_time: optional_timestamptz(row.get(7)?),
            actual_departure_time: optional_timestamptz(row.get(8)?),
            cancellation: row.get(9)?,
            stop_point_ref: row.get(10)?,
        })
    }
}

pub struct VehicleJourneyAppend {
    pub vehicle_journey_row: VehicleJourneyRow,
    pub estimated_call_rows: Vec<EstimatedCallRow>,
    pub recorded_call_rows: Vec<RecordedCallRow>,
}

impl VehicleJourneyAppend {
    fn new(journey: EstimatedVehicleJourney, version: u32) -> Option<Self> {
        // This is long for an annoying technical reason - we want to move out from `journey`,
        // and can only do so once, so we can't do the obvious thing and split it into three functions
        // with one for each row type

        // 3 candidates for id
        let id = journey
            .dated_vehicle_journey_ref
            .map(|r| r.value)
            .or_else(|| {
                journey
                    .framed_vehicle_journey_ref
                    .map(|r| r.dated_vehicle_journey_ref)
            })
            .or_else(|| journey.block_ref.map(|r| r.value))?;
        let data_source = journey.data_source;
        let recorded_at_time = journey.recorded_at_time;

        let estimated_call_rows: Vec<_> = journey
            .estimated_calls
            .map(|et| et.estimated_call)
            .unwrap_or_default()
            .into_iter()
            .map(|et| EstimatedCallRow {
                vehicle_journey_id: id.clone(),
                version,
                data_source: data_source.clone(),
                recorded_at_time,
                order: et.order,
                aimed_arrival_time: et.aimed_arrival_time,
                aimed_departure_time: et.aimed_departure_time,
                expected_arrival_time: et.expected_arrival_time,
                expected_departure_time: et.expected_departure_time,
                cancellation: et.cancellation.unwrap_or(false),
                stop_point_ref: et.stop_point_ref.value,
            })
            .collect();

        let recorded_call_rows: Vec<_> = journey
            .recorded_calls
            .map(|rc| rc.recorded_call)
            .unwrap_or_default()
            .into_iter()
            .map(|rc| RecordedCallRow {
                vehicle_journey_id: id.clone(),
                version,
                data_source: data_source.clone(),
                recorded_at_time,
                order: rc.order,
                aimed_arrival_time: rc.aimed_arrival_time,
                aimed_departure_time: rc.aimed_departure_time,
                actual_arrival_time: rc.actual_arrival_time,
                actual_departure_time: rc.aimed_departure_time,
                cancellation: rc.cancellation.unwrap_or(false),
                stop_point_ref: rc.stop_point_ref.value,
            })
            .collect();

        let vehicle_journey_row = VehicleJourneyRow {
            id,
            version,
            data_source,
            recorded_at_time,
            cancellation: journey.cancellation.unwrap_or(false),
            extra_journey: journey.extra_journey.unwrap_or(false),
            line_ref: journey.line_ref.value,
            direction_ref: journey.direction_ref.value,
            // Often absent, consult calls as a fallback
            // TODO: This logic may be flawed, since we're not comparing `order` across call types
            destination_ref: journey.destination_ref.map(|r| r.value).or_else(|| {
                estimated_call_rows
                    .iter()
                    .max_by_key(|ec| ec.order)
                    .map(|ec| ec.stop_point_ref.clone())
                    .or_else(|| {
                        recorded_call_rows
                            .iter()
                            .max_by_key(|rc| rc.order)
                            .map(|rc| rc.stop_point_ref.clone())
                    })
            }),
            // Often absent, consult calls as a fallback
            // TODO: This logic may be flawed, since we're not comparing `order` across call types
            origin_ref: journey.origin_ref.and_then(|r| r.value).or_else(|| {
                recorded_call_rows
                    .iter()
                    .min_by_key(|rc| rc.order)
                    .map(|rc| rc.stop_point_ref.clone())
                    .or_else(|| {
                        estimated_call_rows
                            .iter()
                            .min_by_key(|ec| ec.order)
                            .map(|ec| ec.stop_point_ref.clone())
                    })
            }),
            started: !recorded_call_rows.is_empty()
                || estimated_call_rows
                    .first()
                    .and_then(|ec| ec.aimed_arrival_time.or(ec.aimed_departure_time))
                    .map(|dt| dt > Utc::now())
                    .unwrap_or(false),
            finished: estimated_call_rows.is_empty(),
        };

        if vehicle_journey_row.started {
            Some(VehicleJourneyAppend {
                vehicle_journey_row,
                estimated_call_rows,
                recorded_call_rows,
            })
        } else {
            None
        }
    }
}

pub fn vehicle_journeys(
    response: SiriETResponse,
    version: u32,
) -> impl Iterator<Item = VehicleJourneyAppend> {
    response
        .siri
        .service_delivery
        .estimated_timetable_delivery
        .into_iter()
        .flat_map(move |timetable| {
            timetable
                .estimated_journey_version_frame
                .into_iter()
                .flat_map(move |frame| {
                    frame
                        .estimated_vehicle_journey
                        .into_iter()
                        .filter_map(move |journey| VehicleJourneyAppend::new(journey, version))
                })
        })
}

#[cfg(test)]
mod tests {
    use crate::entur_data::{EstimatedCallRow, RecordedCallRow, RowType, VehicleJourneyRow};
    use chrono::{DateTime, FixedOffset, TimeDelta};
    use duckdb::Connection;
    use std::ops::Sub;

    fn setup_db() -> duckdb::Result<Connection> {
        let schema = include_str!("schema.sql");
        let db = Connection::open_in_memory()?;
        db.execute_batch(schema)?;
        Ok(db)
    }

    #[test]
    fn vehicle_journy_row_roundtrip() -> anyhow::Result<()> {
        let row = VehicleJourneyRow {
            id: "id".to_string(),
            version: 3,
            data_source: "data_source".to_string(),
            recorded_at_time: Default::default(),
            cancellation: true,
            extra_journey: false,
            line_ref: "line_ref".to_string(),
            direction_ref: "direction_ref".to_string(),
            destination_ref: Some("destination_ref".to_string()),
            origin_ref: Some("origin_ref".to_string()),
            started: true,
            finished: false,
        };

        let db = setup_db()?;
        row.clone()
            .append_with(&mut db.appender("vehicle_journey")?)?;

        let r = db.query_row(
            "select * from vehicle_journey",
            [],
            VehicleJourneyRow::map_from,
        )?;
        assert_eq!(r, row);
        Ok(())
    }

    #[test]
    fn recorded_call_row_roundtrip() -> anyhow::Result<()> {
        let now: DateTime<FixedOffset> = Default::default();
        let row = RecordedCallRow {
            vehicle_journey_id: "vehicle_journey_id".to_string(),
            version: 3,
            data_source: "data_source".to_string(),
            recorded_at_time: Default::default(),
            order: 1,
            aimed_arrival_time: Some(now.sub(TimeDelta::new(1, 0).unwrap())),
            aimed_departure_time: None,
            actual_arrival_time: Some(now.sub(TimeDelta::new(2, 0).unwrap())),
            actual_departure_time: Some(now.sub(TimeDelta::new(3, 0).unwrap())),
            cancellation: true,
            stop_point_ref: "stop_point_ref".to_string(),
        };

        let db = setup_db()?;
        row.clone()
            .append_with(&mut db.appender("recorded_call")?)?;

        let r = db.query_row("select * from recorded_call", [], RecordedCallRow::map_from)?;
        assert_eq!(r, row);
        Ok(())
    }

    #[test]
    fn estimated_call_row_roundtrip() -> anyhow::Result<()> {
        let now: DateTime<FixedOffset> = Default::default();
        let row = EstimatedCallRow {
            vehicle_journey_id: "vehicle_journey_id".to_string(),
            version: 2,
            data_source: "data_source".to_string(),
            recorded_at_time: Default::default(),
            order: 1,
            aimed_arrival_time: Some(now.sub(TimeDelta::new(1, 0).unwrap())),
            aimed_departure_time: None,
            expected_arrival_time: Some(now.sub(TimeDelta::new(2, 0).unwrap())),
            expected_departure_time: Some(now.sub(TimeDelta::new(3, 0).unwrap())),
            cancellation: true,
            stop_point_ref: "stop_point_ref".to_string(),
        };

        let db = setup_db()?;
        row.clone()
            .append_with(&mut db.appender("estimated_call")?)?;

        let r = db.query_row(
            "select * from estimated_call",
            [],
            EstimatedCallRow::map_from,
        )?;
        assert_eq!(r, row);
        Ok(())
    }
}
