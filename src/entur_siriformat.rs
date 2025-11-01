// This file documents the known attributes of the siri et format used by entur, or at least as much
// as I can be bothered defining. Not all the fields will be used, and that's fine. I don't know yet
// what I will need, so I decided to dive into everything now, so that when I discover that there's
// something missing, I will already have discovered it in the right part of the response.
#![allow(dead_code)]

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SiriETRespons {
    siri: Siri
}

#[derive(Deserialize, Debug)]
struct Siri {
    #[serde(rename = "ServiceDelivery")]
    service_delivery: ServiceDelivery,
    version: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct ServiceDelivery {
    estimated_timetable_delivery: Vec<EstimatedTimetableDelivery>,
    producer_ref: StringValue,
    response_timestamp: DateTime<FixedOffset>
}

#[derive(Deserialize, Debug)]
struct EstimatedTimetableDelivery {
    version: String,
    #[serde(rename = "ResponseTimestamp")]
    response_timestamp: DateTime<FixedOffset>,
    #[serde(rename = "EstimatedJourneyVersionFrame")]
    estimated_journey_version_frame: Vec<EstimatedJourneyVersionFrame>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct EstimatedJourneyVersionFrame {
    estimated_vehicle_journey: Vec<EstimatedVehicleJourney>,
    recorded_at_time: DateTime<FixedOffset>
}

/// A journey, identified by a selection of some fields. Future stops are in `estimated_calls` and past stops are in `recorded_calls`.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct EstimatedVehicleJourney {
    /// An identifier of this journey, one possibility among `framed_vehicle_journey_ref` and `dated_vehicle_journey_ref`
    block_ref: Option<StringValue>,
    /// True when the journey has been cancelled in its entirety
    cancellation: Option<bool>,
    /// ATB, RUT, VYG, BNR, ...
    data_source: String,
    /// An identifier, in the initial dataset from entur this is either present, _or_ we get `framed_vehicle_journey_ref`, not both
    dated_vehicle_journey_ref: Option<StringValue>,
    destination_aimed_arrival_time: Option<DateTime<FixedOffset>>,
    destination_display_at_origin: Option<Vec<StringValue>>,
    destination_name: Option<Vec<StringValue>>,
    destination_ref: Option<StringValue>,
    direction_name: Option<Vec<StringValue>>,
    direction_ref: StringValue,
    estimated_calls: Option<EstimatedCalls>,
    /// True when this journey is a replacement for some other journey (eg. buss for tog)
    extra_journey: Option<bool>,
    /// An identifier, in the initial dataset from entur this is either present, _or_ we get `dated_vehicle_journey_ref`, not both
    ///
    /// When this is set, I've found `framed_vehicle_journey_ref.dated_vehicle_journey_ref` to be equal to `block_ref`
    framed_vehicle_journey_ref: Option<FramedVehicleJourneyRef>,
    /// True when estimated_calls + recorded_calls is covers the complete schedule/route (?)
    is_complete_stop_sequence: Option<bool>,
    journey_note: Option<Vec<LocalizedString>>,
    journey_pattern_name: Option<StringValue>,
    journey_pattern_ref: Option<StringValue>,
    line_ref: StringValue,
    monitored: bool,
    occupancy: Option<String>,
    operator_ref: Option<StringValue>,
    origin_aimed_departure_time: Option<DateTime<FixedOffset>>,
    origin_name: Option<Vec<StringValue>>,
    origin_ref: Option<OptionalStringValue>,
    prediction_inaccurate: Option<bool>,
    product_category_ref: Option<StringValue>,
    published_line_name: Option<Vec<StringValue>>,
    recorded_at_time: DateTime<FixedOffset>,
    recorded_calls: Option<RecordedCalls>,
    service_feature_ref: Option<Vec<StringValue>>,
    vehicle_mode: Option<Vec<String>>,
    vehicle_ref: Option<StringValue>,
    via: Option<Vec<Via>>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Via {
    place_name: Vec<StringValue>
}

#[derive(Deserialize, Debug)]
struct LocalizedString {
    lang: String,
    value: String
}

#[derive(Deserialize, Debug)]
struct OptionalStringValue {
    value: Option<String>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct FramedVehicleJourneyRef {
    data_frame_ref: DataFrameRef,
    dated_vehicle_journey_ref: String
}

#[derive(Deserialize, Debug)]
struct DataFrameRef {
    value: NaiveDate
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct RecordedCalls {
    recorded_call: Vec<RecordedCall>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct RecordedCall {
    actual_arrival_time: Option<DateTime<FixedOffset>>,
    actual_departure_time: Option<DateTime<FixedOffset>>,
    aimed_arrival_time: Option<DateTime<FixedOffset>>,
    aimed_departure_time: Option<DateTime<FixedOffset>>,
    arrival_platform_name: Option<StringValue>,
    cancellation: Option<bool>,
    departure_platform_name: Option<StringValue>,
    expected_arrival_time: Option<DateTime<FixedOffset>>,
    expected_departure_time: Option<DateTime<FixedOffset>>,
    occupancy: Option<String>,
    order: u16,
    prediction_inaccurate: Option<bool>,
    stop_point_name: Option<Vec<StringValue>>,
    stop_point_ref: StringValue,
    visit_number: Option<u16>,
    via: Option<Vec<Via>>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct EstimatedCalls {
    estimated_call: Vec<EstimatedCall>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct EstimatedCall {
    aimed_arrival_time: Option<DateTime<FixedOffset>>,
    aimed_departure_time: Option<DateTime<FixedOffset>>,
    arrival_status: Option<String>,
    arrival_stop_assignment: Option<StopAssignment>,
    /// This particular call/stop is cancelled, but not necessary the journey.
    cancellation: Option<bool>,
    departure_boarding_activity: Option<String>,
    departure_platform_name: Option<StringValue>,
    departure_status: Option<String>,
    departure_stop_assignment: Option<StopAssignment>,
    destination_display: Option<Vec<StringValue>>,
    expected_arrival_time: Option<DateTime<FixedOffset>>,
    expected_arrival_prediction_quality: Option<PredictionQuality>,
    expected_departure_time: Option<DateTime<FixedOffset>>,
    occupancy: Option<String>,
    order: u16,
    prediction_inaccurate: Option<bool>,
    request_stop: Option<bool>,
    stop_point_name: Option<Vec<StringValue>>,
    stop_point_ref: StringValue,
    timing_point: Option<bool>,
    visit_number: Option<u16>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct PredictionQuality {
    prediction_level: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct StopAssignment {
    actual_quay_ref: Option<StringValue>,
    aimed_quay_ref: StringValue,
    expected_quay_ref: StringValue,
}


#[derive(Deserialize, Debug)]
struct StringValue {
    value: String
}