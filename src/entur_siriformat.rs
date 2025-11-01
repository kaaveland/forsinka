// This file documents the known attributes of the siri et format used by entur, or at least as much
// as I can be bothered defining. Not all the fields will be used, and that's fine. I don't know yet
// what I will need, so I decided to dive into everything now, so that when I discover that there's
// something missing, I will already have discovered it in the right part of the response.
#![allow(dead_code)]

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct SiriETResponse {
    pub siri: Siri
}

#[derive(Deserialize, Debug)]
pub struct Siri {
    #[serde(rename = "ServiceDelivery")]
    pub service_delivery: ServiceDelivery,
    pub version: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct ServiceDelivery {
    pub estimated_timetable_delivery: Vec<EstimatedTimetableDelivery>,
    pub producer_ref: StringValue,
    pub response_timestamp: DateTime<FixedOffset>
}

#[derive(Deserialize, Debug)]
pub struct EstimatedTimetableDelivery {
    pub version: String,
    #[serde(rename = "ResponseTimestamp")]
    pub response_timestamp: DateTime<FixedOffset>,
    #[serde(rename = "EstimatedJourneyVersionFrame")]
    pub estimated_journey_version_frame: Vec<EstimatedJourneyVersionFrame>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct EstimatedJourneyVersionFrame {
    pub estimated_vehicle_journey: Vec<EstimatedVehicleJourney>,
    pub recorded_at_time: DateTime<FixedOffset>
}

/// A journey, identified by a selection of some fields. Future stops are in `estimated_calls` and past stops are in `recorded_calls`.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct EstimatedVehicleJourney {
    /// An identifier of this journey, one possibility among `framed_vehicle_journey_ref` and `dated_vehicle_journey_ref`
    pub block_ref: Option<StringValue>,
    /// True when the journey has been cancelled in its entirety
    pub cancellation: Option<bool>,
    /// ATB, RUT, VYG, BNR, ...
    pub data_source: String,
    /// An identifier, in the initial dataset from entur this is either present, _or_ we get `framed_vehicle_journey_ref`, not both
    pub dated_vehicle_journey_ref: Option<StringValue>,
    pub destination_aimed_arrival_time: Option<DateTime<FixedOffset>>,
    pub destination_display_at_origin: Option<Vec<StringValue>>,
    pub destination_name: Option<Vec<StringValue>>,
    pub destination_ref: Option<StringValue>,
    pub direction_name: Option<Vec<StringValue>>,
    pub direction_ref: StringValue,
    pub estimated_calls: Option<EstimatedCalls>,
    /// True when this journey is a replacement for some other journey (eg. buss for tog)
    pub extra_journey: Option<bool>,
    /// An identifier, in the initial dataset from entur this is either present, _or_ we get `dated_vehicle_journey_ref`, not both
    ///
    /// When this is set, I've found `framed_vehicle_journey_ref.dated_vehicle_journey_ref` to be equal to `block_ref`
    pub framed_vehicle_journey_ref: Option<FramedVehicleJourneyRef>,
    /// True when estimated_calls + recorded_calls is covers the complete schedule/route (?)
    pub is_complete_stop_sequence: Option<bool>,
    pub journey_note: Option<Vec<LocalizedString>>,
    pub journey_pattern_name: Option<StringValue>,
    pub journey_pattern_ref: Option<StringValue>,
    pub line_ref: StringValue,
    pub monitored: Option<bool>,
    pub occupancy: Option<String>,
    pub operator_ref: Option<StringValue>,
    pub origin_aimed_departure_time: Option<DateTime<FixedOffset>>,
    pub origin_name: Option<Vec<StringValue>>,
    pub origin_ref: Option<OptionalStringValue>,
    pub prediction_inaccurate: Option<bool>,
    pub product_category_ref: Option<StringValue>,
    pub published_line_name: Option<Vec<StringValue>>,
    pub recorded_at_time: DateTime<FixedOffset>,
    pub recorded_calls: Option<RecordedCalls>,
    pub service_feature_ref: Option<Vec<StringValue>>,
    pub vehicle_mode: Option<Vec<String>>,
    pub vehicle_ref: Option<StringValue>,
    pub via: Option<Vec<Via>>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Via {
    pub place_name: Vec<StringValue>
}

#[derive(Deserialize, Debug)]
pub struct LocalizedString {
    pub lang: Option<String>,
    pub value: String
}

#[derive(Deserialize, Debug)]
pub struct OptionalStringValue {
    pub value: Option<String>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct FramedVehicleJourneyRef {
    pub data_frame_ref: DataFrameRef,
    pub dated_vehicle_journey_ref: String
}

#[derive(Deserialize, Debug)]
pub struct DataFrameRef {
    pub value: NaiveDate
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct RecordedCalls {
    pub recorded_call: Vec<RecordedCall>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct RecordedCall {
    pub actual_arrival_time: Option<DateTime<FixedOffset>>,
    pub actual_departure_time: Option<DateTime<FixedOffset>>,
    pub aimed_arrival_time: Option<DateTime<FixedOffset>>,
    pub aimed_departure_time: Option<DateTime<FixedOffset>>,
    pub arrival_platform_name: Option<StringValue>,
    pub cancellation: Option<bool>,
    pub departure_platform_name: Option<StringValue>,
    pub expected_arrival_time: Option<DateTime<FixedOffset>>,
    pub expected_departure_time: Option<DateTime<FixedOffset>>,
    pub occupancy: Option<String>,
    pub order: u16,
    pub prediction_inaccurate: Option<bool>,
    pub stop_point_name: Option<Vec<StringValue>>,
    pub stop_point_ref: StringValue,
    pub visit_number: Option<u16>,
    pub via: Option<Vec<Via>>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct EstimatedCalls {
    pub estimated_call: Vec<EstimatedCall>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct EstimatedCall {
    pub aimed_arrival_time: Option<DateTime<FixedOffset>>,
    pub aimed_departure_time: Option<DateTime<FixedOffset>>,
    pub arrival_status: Option<String>,
    pub arrival_stop_assignment: Option<StopAssignment>,
    /// This particular call/stop is cancelled, but not necessary the journey.
    pub cancellation: Option<bool>,
    pub departure_boarding_activity: Option<String>,
    pub departure_platform_name: Option<StringValue>,
    pub departure_status: Option<String>,
    pub departure_stop_assignment: Option<StopAssignment>,
    pub destination_display: Option<Vec<StringValue>>,
    pub expected_arrival_time: Option<DateTime<FixedOffset>>,
    pub expected_arrival_prediction_quality: Option<PredictionQuality>,
    pub expected_departure_time: Option<DateTime<FixedOffset>>,
    pub occupancy: Option<String>,
    pub order: u16,
    pub prediction_inaccurate: Option<bool>,
    pub request_stop: Option<bool>,
    pub stop_point_name: Option<Vec<StringValue>>,
    pub stop_point_ref: StringValue,
    pub timing_point: Option<bool>,
    pub visit_number: Option<u16>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct PredictionQuality {
    pub prediction_level: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct StopAssignment {
    pub actual_quay_ref: Option<StringValue>,
    pub aimed_quay_ref: StringValue,
    pub expected_quay_ref: StringValue,
}


#[derive(Deserialize, Debug)]
pub struct StringValue {
    pub value: String
}