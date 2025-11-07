use crate::db::StopRow;
use crate::entur_siriformat::EstimatedVehicleJourney;
use chrono::{DateTime, FixedOffset};
use fxhash::{FxHashMap, FxHashSet};
use ordered_float::OrderedFloat;

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
}

impl Stops {
    fn get(&self, stop_point_ref: &StopPointRef) -> Option<&Stop> {
        self.stops.get(stop_point_ref)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Journey<'a> {
    last_update: DateTime<FixedOffset>,
    journey_id: JourneyId,
    data_source: String,
    line_ref: String,
    cancelled: bool,
    finished: bool,
    origin: &'a Stop,
    destination: &'a Stop,
    prev_stop: &'a Stop,
    next_stop: Option<&'a Stop>,
    prev_stop_planned_time: DateTime<FixedOffset>,
    prev_stop_actual_time: DateTime<FixedOffset>,
    next_stop_planned_time: Option<DateTime<FixedOffset>>,
    to_visit: FxHashSet<&'a str>,
}

impl<'a> Journey<'a> {
    fn new(
        stops: &'a Stops,
        journey_id: JourneyId,
        journey: EstimatedVehicleJourney,
    ) -> Option<Self> {
        let last_update = journey.recorded_at_time;
        // This throws out journeys that haven't started, which is okay for us.
        let recorded = journey.recorded_calls?.recorded_call;
        let estimated = journey
            .estimated_calls
            .map(|ec| ec.estimated_call)
            .unwrap_or_default();
        let finished = estimated.is_empty();
        let data_source = journey.data_source;
        let first = recorded.first()?;
        let origin_id = StopPointRef(first.stop_point_ref.value.clone());
        let prev = recorded.last()?;
        // This throws out the whole journey if we don't have any actual or planned times for the previous stop
        let prev_stop_planned_time = prev
            .aimed_arrival_time
            .or_else(|| prev.aimed_departure_time)?;
        let prev_stop_actual_time = prev
            .actual_arrival_time
            .or_else(|| prev.actual_departure_time)?;
        let prev_stop = stops
            .stops
            .get(&StopPointRef(prev.stop_point_ref.value.clone()))?;

        let (next_stop, next_stop_planned_time) = estimated
            .first()
            .and_then(|first| {
                Some((
                    stops
                        .stops
                        .get(&StopPointRef(first.stop_point_ref.value.clone()))?,
                    first
                        .aimed_arrival_time
                        .or_else(|| first.aimed_departure_time)?
                        .clone(),
                ))
            })
            .unzip();

        // This block throws out journeys to stops we don't know about
        let origin = stops.get(&origin_id)?;
        let last = estimated
            .last()
            .map(|ec| &ec.stop_point_ref.value)
            .or_else(|| {
                recorded
                    .iter()
                    .last()
                    .map(|dest| &dest.stop_point_ref.value)
            })?;
        let destination_id = StopPointRef(last.clone());
        let destination = stops.get(&destination_id)?;
        // This throws out only stops we can't find, not the actual journey
        let to_visit: FxHashSet<_> = estimated
            .into_iter()
            .filter(|ec| !ec.cancellation.unwrap_or(false))
            .map(|ec| StopPointRef(ec.stop_point_ref.value))
            .filter_map(|id| stops.get(&id))
            .map(|stop| stop.name.as_str())
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
}

pub struct Journeys<'a> {
    journeys: FxHashMap<JourneyId, Journey<'a>>,
}

impl<'a> Journeys<'a> {
    pub fn by_id(&'a self, id: &JourneyId) -> Option<&'a Journey<'a>> {
        self.journeys.get(id)
    }

    pub fn by_visits(&'a self, stop_name: &str) -> Vec<&'a Journey<'a>> {
        self.journeys
            .values()
            .filter(|journey| journey.to_visit.contains(stop_name))
            .collect()
    }

    pub fn train_journeys(&'a self) -> Vec<&'a Journey<'a>> {
        let train_ds = &["VYG", "BNR", "SJN", "GOA", "FLY", "FLT"];
        self.journeys
            .values()
            .filter(|journey| train_ds.contains(&journey.data_source.as_str()))
            .collect()
    }

    pub fn new(stops: &'a Stops, journeys: impl Iterator<Item = EstimatedVehicleJourney>) -> Self {
        let journeys = journeys
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
                Some((
                    JourneyId(id.clone()),
                    Journey::new(stops, JourneyId(id), journey_row)?,
                ))
            })
            .collect();
        Self { journeys }
    }

    pub fn merge_from(&mut self, other: Journeys<'a>, cutoff: DateTime<FixedOffset>) {
        for (id, journey) in other.journeys.into_iter() {
            self.journeys.insert(id, journey);
        }
        self.journeys
            .retain(|id, journey| journey.last_update > cutoff);
    }
}
