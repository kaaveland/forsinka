use askama::Template;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::Europe::Oslo;
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

#[derive(Serialize)]
pub struct Healthy {
    pub last_successful_sync: Option<u32>,
    pub next_sync_attempt: Option<u32>,
    pub healthy: bool,
}

impl IntoResponse for Healthy {
    fn into_response(self) -> Response {
        if self.healthy {
            (StatusCode::OK, Json(self)).into_response()
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(self)).into_response()
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
