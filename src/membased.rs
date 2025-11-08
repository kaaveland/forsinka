use crate::api::{JourneyDelay, TrainJourney};
use crate::db::StopRow;
use crate::entur_siriformat::{EstimatedCall, EstimatedVehicleJourney, RecordedCall};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use fxhash::{FxHashMap, FxHashSet};
use ordered_float::OrderedFloat;
use tracing::info;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct JourneyId(String);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Stop {
    name: String,
    lat: Option<OrderedFloat<f32>>,
    lon: Option<OrderedFloat<f32>>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct StopPointRef(String);

#[derive(Clone)]
pub struct Stops {
    stops: FxHashMap<StopPointRef, Stop>,
}

impl Stops {
    pub fn new(stops: Vec<StopRow>) -> Self {
        let stops = stops
            .iter()
            .map(|row| {
                let id = StopPointRef(row.stop_point_ref.to_string());
                (
                    id,
                    Stop {
                        name: row.name.to_string(),
                        lat: row.lat,
                        lon: row.lon,
                    },
                )
            })
            .collect();
        Self { stops }
    }
    pub fn stop_names(&self) -> impl Iterator<Item = String> {
        let refs: FxHashSet<_> = self.stops.values().map(|stop| &stop.name).collect();
        refs.into_iter().cloned()
    }
}

impl Stops {
    fn get(&self, stop_point_ref: &StopPointRef) -> Option<&Stop> {
        self.stops.get(stop_point_ref)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Journey {
    last_update: DateTime<FixedOffset>,
    journey_id: JourneyId,
    data_source: String,
    line_ref: String,
    cancelled: bool,
    finished: bool,
    origin: Stop,
    destination: Stop,
    prev_stop: Stop,
    next_stop: Option<Stop>,
    prev_stop_planned_time: DateTime<FixedOffset>,
    prev_stop_actual_time: DateTime<FixedOffset>,
    next_stop_planned_time: Option<DateTime<FixedOffset>>,
    to_visit: FxHashSet<String>,
}

impl TryFrom<&EstimatedCall> for Stop {
    type Error = ();

    fn try_from(value: &EstimatedCall) -> Result<Self, Self::Error> {
        let v = value
            .stop_point_name
            .as_ref()
            .and_then(|seg| seg.first())
            .map(|name| Stop {
                name: name.value.clone(),
                lat: None,
                lon: None,
            });
        if let Some(stop) = v {
            Ok(stop)
        } else {
            Err(())
        }
    }
}

impl TryFrom<&RecordedCall> for Stop {
    type Error = ();

    fn try_from(value: &RecordedCall) -> Result<Self, Self::Error> {
        let v = value
            .stop_point_name
            .as_ref()
            .and_then(|seg| seg.first())
            .map(|name| Stop {
                name: name.value.clone(),
                lat: None,
                lon: None,
            });
        if let Some(stop) = v {
            Ok(stop)
        } else {
            Err(())
        }
    }
}

impl From<&RecordedCall> for StopPointRef {
    fn from(value: &RecordedCall) -> Self {
        StopPointRef(value.stop_point_ref.value.clone())
    }
}

impl From<&EstimatedCall> for StopPointRef {
    fn from(value: &EstimatedCall) -> Self {
        StopPointRef(value.stop_point_ref.value.clone())
    }
}

fn stop_with_fallback<C: TryInto<Stop> + Into<StopPointRef> + Copy>(
    stops: &Stops,
    call: C,
) -> Option<Stop> {
    let id: StopPointRef = call.into();
    let primary = stops.get(&id).cloned();
    primary.or_else(|| call.try_into().ok())
}

impl Journey {
    fn new(stops: &Stops, journey_id: JourneyId, journey: EstimatedVehicleJourney) -> Option<Self> {
        let last_update = journey.recorded_at_time;
        // This throws out journeys that haven't started, which is okay for us.
        let recorded = journey.recorded_calls?.recorded_call;
        let estimated = journey
            .estimated_calls
            .map(|ec| ec.estimated_call)
            .unwrap_or_default();
        let finished = estimated.is_empty();
        let data_source = journey.data_source;
        let first_recorded = recorded.first()?;

        let prev = recorded.last()?;
        // This throws out the whole journey if we don't have any actual or planned times for the previous stop
        let prev_stop_planned_time = prev.aimed_arrival_time.or(prev.aimed_departure_time)?;
        let prev_stop_actual_time = prev
            .actual_arrival_time
            .or(prev.actual_departure_time)
            .or(prev.expected_arrival_time)
            .or(prev.expected_departure_time)?;
        let prev_stop: Stop = stop_with_fallback(stops, prev)?;

        let (next_stop, next_stop_planned_time) = estimated
            .first()
            .and_then(|first_estimated| {
                Some((
                    stop_with_fallback(stops, first_estimated)?,
                    first_estimated
                        .aimed_arrival_time
                        .or(first_estimated.aimed_departure_time)?,
                ))
            })
            .unzip();

        let origin = stop_with_fallback(stops, first_recorded)?;

        let destination = estimated
            .last()
            .and_then(|ec| stop_with_fallback(stops, ec))
            .or_else(|| {
                recorded
                    .last()
                    .and_then(|dest| stop_with_fallback(stops, dest))
            })?;

        // This throws out only stops we can't find, not the actual journey
        let to_visit: FxHashSet<_> = estimated
            .into_iter()
            .filter_map(|est| stop_with_fallback(stops, &est).map(|stop| stop.name))
            .collect();
        let line_ref = journey.line_ref.value;

        Some(Self {
            last_update,
            journey_id,
            data_source,
            line_ref,
            cancelled: journey.cancellation.unwrap_or(false),
            finished,
            origin,
            destination,
            prev_stop,
            next_stop,
            prev_stop_planned_time,
            prev_stop_actual_time,
            next_stop_planned_time,
            to_visit,
        })
    }

    fn recorded_delay_seconds(&self) -> i32 {
        (self.prev_stop_actual_time - self.prev_stop_planned_time).as_seconds_f32() as i32
    }

    fn possibly_stuck(&self) -> bool {
        if let Some(next) = self.next_stop_planned_time {
            let planned_travel_time = next - self.prev_stop_planned_time;
            let cushion = planned_travel_time + TimeDelta::minutes(8);
            let cutoff = self.prev_stop_planned_time + cushion;
            Utc::now() > cutoff
        } else {
            // At last stop
            false
        }
    }
}

#[derive(Clone)]
pub struct Journeys {
    journeys: FxHashMap<JourneyId, Journey>,
}

impl Journeys {
    pub fn by_visits(&self, stop_name: &str) -> Vec<&Journey> {
        self.journeys
            .values()
            .filter(|journey| journey.to_visit.contains(stop_name))
            .collect()
    }

    pub fn train_journeys(&self) -> Vec<&Journey> {
        let train_ds = &["VYG", "BNR", "SJN", "GOA", "FLY", "FLT"];
        self.journeys
            .values()
            .filter(|journey| train_ds.contains(&journey.data_source.as_str()))
            .collect()
    }

    pub fn new(stops: &Stops, journeys: impl Iterator<Item = EstimatedVehicleJourney>) -> Self {
        let mut journeys: Vec<_> = journeys.collect();
        journeys.sort_by_key(|journey| journey.recorded_at_time);
        let journeys = journeys
            .into_iter()
            .filter_map(|journey_row| {
                let id = journey_row
                    .dated_vehicle_journey_ref
                    .as_ref()
                    .map(|r| r.value.as_str())
                    .or_else(|| {
                        journey_row
                            .framed_vehicle_journey_ref
                            .as_ref()
                            .map(|r| r.dated_vehicle_journey_ref.as_str())
                    })
                    .or_else(|| journey_row.block_ref.as_ref().map(|r| r.value.as_str()))
                    .map(|id| id.to_string())?;
                let mapped = Journey::new(stops, JourneyId(id.clone()), journey_row);
                Some((JourneyId(id.clone()), mapped?))
            })
            .collect();
        Self { journeys }
    }

    pub fn merge_from(&mut self, other: Journeys) {
        for (id, journey) in other.journeys.into_iter() {
            self.journeys.insert(id, journey);
        }
    }

    pub fn expire(&mut self, cutoff: DateTime<FixedOffset>) {
        self.journeys
            .retain(|_, journey| journey.last_update > cutoff);
    }

    pub fn len(&self) -> usize {
        self.journeys.len()
    }
}

impl From<Journey> for JourneyDelay {
    fn from(value: Journey) -> Self {
        let recorded_delay_seconds = value.recorded_delay_seconds();

        Self {
            vehicle_journey_id: value.journey_id.0,
            line_ref: value.line_ref,
            last_stop_name: value.prev_stop.name,
            aimed_last_stop_time: value.prev_stop_planned_time,
            actual_last_stop_time: value.prev_stop_actual_time,
            recorded_delay_seconds,
            next_stop_name: value.next_stop.map(|s| s.name),
            aimed_next_stop_time: value.next_stop_planned_time,
        }
    }
}

impl From<Journey> for TrainJourney {
    fn from(value: Journey) -> Self {
        let recorded_delay_seconds = value.recorded_delay_seconds();
        let possibly_stuck = value.possibly_stuck();
        Self {
            vehicle_journey_id: value.journey_id.0,
            line_ref: format!(
                "{}: {} to {}",
                value.line_ref.split(':').next_back().unwrap(),
                value.origin.name.trim_end_matches(" stasjon"),
                value.destination.name.trim_end_matches(" stasjon")
            ),
            cancellation: value.cancelled,
            data_source: value.data_source,
            stop_name: value.prev_stop.name,
            next_stop_name: value.next_stop.map(|s| s.name),
            aimed_time: value.prev_stop_planned_time,
            actual_time: value.prev_stop_actual_time,
            delay_seconds: recorded_delay_seconds,
            next_stop_time: value.next_stop_planned_time,
            departed: true,
            possibly_stuck,
        }
    }
}
