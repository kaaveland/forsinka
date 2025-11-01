use std::fs;
use chrono::{DateTime, FixedOffset, Offset};
use tracing::info;
use uuid::Uuid;
use reqwest::Client;
use crate::entur_siriformat;
use crate::entur_siriformat::SiriETResponse;

pub const ENTUR_API_URL: &str = "https://api.entur.io/realtime/v1/rest/et";

async fn fetch_siri(
    client: &Client,
    url: &str,
    requestor_id: &str
) -> anyhow::Result<SiriETResponse> {
    info!("Poll {url} with requestorId={requestor_id}");
    Ok(client.get(url)
        .query(&[("requestorId", requestor_id)])
        .header("Accept", "application/json")
        .send().await?.json().await?)
}

pub async fn fetch_initial_data(static_data: &Option<String>, api_url: &String, me: &Uuid, client: &Client) -> anyhow::Result<SiriETResponse> {
    if let Some(path) = static_data {
        let content = fs::read(path)?;
        Ok(serde_json::from_slice(&content)?)
    } else {
        fetch_siri(
            client, api_url.as_str(), me.to_string().as_str()
        ).await
    }
}

pub struct VehicleJourney {
    pub id: String,
    pub data_source: String,
    pub recorded_at_time: DateTime<FixedOffset>,
    pub cancellation: bool,
    pub extra_journey: bool,
    pub line_ref: String,
    pub direction_ref: String,
    pub destination_ref: Option<String>,
    pub origin_ref: Option<String>
}

pub fn vehicle_journeys(response: SiriETResponse) -> impl Iterator<Item=VehicleJourney> {
    response.siri.service_delivery.estimated_timetable_delivery.into_iter().flat_map(|timetable| {
        timetable.estimated_journey_version_frame.into_iter().flat_map(|frame| {
            frame.estimated_vehicle_journey.into_iter().filter_map(|journey| {
                Some(VehicleJourney {
                    // 3 candidates for id
                    id: journey.dated_vehicle_journey_ref.map(|r| r.value).or_else(
                        || journey.framed_vehicle_journey_ref.map(|r| r.dated_vehicle_journey_ref)
                    ).or_else(
                        || journey.block_ref.map(|r| r.value)
                    )?,
                    data_source: journey.data_source,
                    recorded_at_time: journey.recorded_at_time,
                    cancellation: journey.cancellation.unwrap_or(false),
                    extra_journey: journey.extra_journey.unwrap_or(false),
                    line_ref: journey.line_ref.value,
                    direction_ref: journey.direction_ref.value,
                    // Consider falling back to last row of journey.estimated_calls / journey.recorded_calls
                    destination_ref: journey.destination_ref.map(|r|r.value),
                    // Consider falling back to first row of journey.estimated_calls / journey.recorded_calls
                    origin_ref: journey.origin_ref.and_then(|r|r.value),
                })
            })
        })
    })
}