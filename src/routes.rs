// Router setup
use crate::handlers;
use crate::server::state::AppState;
use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Router, http};
use http::HeaderValue;
use http::header::CACHE_CONTROL;
use std::time;
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tower_http::cors;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::error;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::root))
        .route("/healthy", get(handlers::healthy))
        .route("/stop/{stop_name}", get(handlers::by_stop_name))
        .route("/stops", get(handlers::stop_names))
        .route("/trains", get(handlers::train_journeys))
        .route("/trains.html", get(handlers::train_journeys_html))
        .nest_service("/static", ServeDir::new("static"))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: BoxError| async {
                    error!("Timed out");
                    (StatusCode::REQUEST_TIMEOUT, "Timed out. Sorry!".to_string())
                }))
                .layer(TimeoutLayer::new(time::Duration::from_millis(500)))
                .layer(
                    CorsLayer::new()
                        .allow_methods([http::Method::GET])
                        .allow_origin(cors::Any),
                ),
        )
        .layer(SetResponseHeaderLayer::if_not_present(
            CACHE_CONTROL,
            HeaderValue::from_static("public, s-maxage=10, max-age=30"),
        ))
        .with_state(state)
}
