// HTTP request handlers
use crate::api::{Healthy, JourneyDelay, TrainJourney, TrainsPage};
use crate::server::infra::WebappError;
use crate::server::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use std::cmp::Reverse;
use tracing::instrument;

pub async fn root() -> impl IntoResponse {
    axum::response::Redirect::to("trains.html")
}

#[instrument(name = "by_stop_name", skip(state))]
pub async fn by_stop_name(
    State(state): State<AppState>,
    Path(stop_name): Path<String>,
) -> Result<Json<Vec<JourneyDelay>>, WebappError> {
    let journeys = state.state.read().unwrap();
    let journeys = journeys.by_visits(stop_name.as_str()).into_iter().cloned();
    Ok(Json(journeys.map(|journey| journey.into()).collect()))
}

#[instrument(name = "train_journeys", skip_all)]
pub async fn train_journeys(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrainJourney>>, WebappError> {
    let journeys = state.state.read().unwrap();
    let mut train_journeys: Vec<TrainJourney> = journeys
        .train_journeys()
        .into_iter()
        .cloned()
        .map(|tj| tj.into())
        .collect();
    train_journeys.sort_by_key(|tj| Reverse((tj.possibly_stuck, tj.delay_seconds)));
    Ok(Json(train_journeys))
}

#[instrument(name = "train_journeys_html", skip_all)]
pub async fn train_journeys_html(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, WebappError> {
    let journeys = state.state.read().unwrap();
    let mut train_journeys: Vec<TrainJourney> = journeys
        .train_journeys()
        .into_iter()
        .cloned()
        .map(|tj| tj.into())
        .collect();
    train_journeys.sort_by_key(|tj| Reverse((tj.possibly_stuck, tj.delay_seconds)));
    Ok(TrainsPage::new(train_journeys, state.assets_path.clone()))
}

pub async fn stop_names(State(state): State<AppState>) -> Result<Json<Vec<String>>, WebappError> {
    Ok(Json(state.stops.stop_names().collect()))
}

pub async fn healthy(State(app_state): State<AppState>) -> Healthy {
    let last_successful_sync = app_state.last_successful_sync.read().ok().map(|last| *last);
    let next_sync_attempt = app_state.next_sync.read().ok().map(|now| *now);
    let healthy = next_sync_attempt
        .and_then(|now| last_successful_sync.map(|then| now - then < 10))
        .unwrap_or(false);

    Healthy {
        last_successful_sync,
        next_sync_attempt,
        healthy,
    }
}
