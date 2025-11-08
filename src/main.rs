use crate::api::JourneyDelay;
use crate::api::TrainJourney;
use crate::api::TrainsPage;
use crate::entur_data::Config;
use crate::entur_siriformat::SiriETResponse;
use crate::membased::{Journeys, Stops};
use askama::Template;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router, http};
use chrono::{Duration, Utc};
use chrono_tz::Europe::Oslo;
use clap::{Parser, Subcommand};
use duckdb::Connection;
use http::HeaderValue;
use http::header::CACHE_CONTROL;
use reqwest::ClientBuilder;
use serde::Serialize;
use std::cmp::Reverse;
use std::ops::Sub;
use std::sync::{Arc, RwLock};
use std::time;
use tokio::signal;
use tokio::sync::watch::{Receiver, Sender};
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tower_http::cors;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{error, info, instrument};
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod api;
mod db;
mod entur_data;
mod entur_siriformat;
mod membased;

#[derive(Parser)]
struct SharedOptions {
    /// Instead of connecting to Entur API, use a static json data file that has been downloaded
    #[arg(short = 's', long = "static-data")]
    static_data: Option<String>,
    /// Retrieve the data from an alternate source. The source must deliver json-data that matches
    /// the Entur API.
    #[arg(short = 'u', long = "api-url", default_value = entur_data::ENTUR_API_URL)]
    api_url: String,
    /// URL or file path to fetch quays.parquet and stops.parquet for geolocating stops
    #[arg(
        long = "parquet-root",
        default_value = "https://kaaveland-bus-eta-data.hel1.your-objectstorage.com/"
    )]
    parquet_root: String,
    /// DuckDB to connect to - uses an inmemory-db if not configured.
    #[arg(short = 'd', long = "db-url")]
    db_url: Option<String>,
    /// requestorId to send to entur api, to fetch only diff since last fetch
    /// The default behaviour is to generate a unique one startup to receive the full dataset the first run
    #[arg(long = "requestor-id")]
    requestor_id: Option<String>,
    /// Number of threads to configure DuckDB with
    #[arg(short = 'j', long = "threads", default_value = "1")]
    threads: u8,
    /// GB of RAM to grant DuckDB
    #[arg(short = 'm', long = "memory-gbs", default_value = "1")]
    memory_gb: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a long-lived http server that continually imports data
    Serve {
        #[command(flatten)]
        shared_options: SharedOptions,
        /// Host the webapp on this particular port
        #[arg(short = 'p', long = "port", default_value = "4500")]
        port: u16,
        /// Check for new data every fetch-interval seconds. If not provided, never refetch.
        #[arg(short = 'i', long = "fetch-interval-seconds")]
        fetch_interval_seconds: Option<u16>,
        #[arg(long = "assets-path", default_value = "/static")]
        assets_path: String,
    },
}

#[derive(Parser)]
#[command(name = "forsinka")]
#[command(about = "Run API to expose current public transit delays in Norway")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Forsinka {
    #[command(subcommand)]
    command: Commands,
}

async fn initial_import(
    args: SharedOptions,
) -> anyhow::Result<(Connection, SiriETResponse, Config)> {
    let me = args
        .requestor_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let client = ClientBuilder::default()
        .connect_timeout(time::Duration::from_millis(1_000))
        .timeout(time::Duration::from_millis(60_000))
        .build()?;

    let db = db::prepare_db(
        &args.db_url,
        &args.parquet_root,
        args.threads,
        args.memory_gb,
    )?;
    let config = Config::new(me, args.api_url.clone(), client, args.static_data);

    let data = entur_data::fetch_data(&config).await?;
    Ok((db, data, config))
}

async fn root() -> impl IntoResponse {
    axum::response::Redirect::to("trains.html")
}

async fn shutdown_signal(terminate_jobs: Sender<bool>) {
    let interrupt = async {
        signal::ctrl_c()
            .await
            .expect("Unable to set signal handler for Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {
            info!("Received Ctrl+C signal");
            terminate_jobs.send(true).ok();
        },
        _ = terminate => {
            info!("Received terminate signal");
            terminate_jobs.send(true).ok();
        },
    }
}

struct WebappError {
    inner: anyhow::Error,
}

impl IntoResponse for WebappError {
    fn into_response(self) -> Response {
        error!("Error: {:?}", self.inner);
        Response::builder()
            .status(500)
            .body("Internal Server Error".into())
            .unwrap()
    }
}

impl<E> From<E> for WebappError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self { inner: err.into() }
    }
}

#[instrument(name = "by_stop_name", skip_all)]
async fn by_stop_name(
    State(state): State<AppState>,
    Path(stop_name): Path<String>,
) -> Result<Json<Vec<JourneyDelay>>, WebappError> {
    let journeys = state.state.read().unwrap();
    let journeys = journeys.by_visits(stop_name.as_str()).into_iter().cloned();
    Ok(Json(journeys.map(|journey| journey.into()).collect()))
}

#[instrument(name = "train_journeys", skip_all)]
async fn train_journeys(
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

const TEMPLATE_ERROR_HTML: &str = r#"<!DOCTYPE html>
<html lang="no">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Feil - forsinka</title>
    <style>
        body { font-family: sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .error { background: white; padding: 40px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); text-align: center; }
        h1 { color: #d32f2f; margin-bottom: 20px; }
        p { color: #666; }
        a { color: #667eea; text-decoration: none; }
    </style>
</head>
<body>
    <div class="error">
        <h1>⚠️ En ukjent feil oppsto</h1>
        <p>Beklager, vi kunne ikke vise siden.</p>
        <p><a href="/trains">Prøv JSON API</a> eller <a href="/">tilbake til forsiden</a></p>
    </div>
</body>
</html>"#;

impl IntoResponse for TrainsPage {
    fn into_response(self) -> Response {
        if let Ok(html) = self.render() {
            axum::response::Html(html).into_response()
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::response::Html(TEMPLATE_ERROR_HTML),
            )
                .into_response()
        }
    }
}

#[instrument(name = "train_journeys_html", skip_all)]
async fn train_journeys_html(
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

async fn stop_names(State(state): State<AppState>) -> Result<Json<Vec<String>>, WebappError> {
    Ok(Json(state.stops.stop_names().collect()))
}

#[derive(Clone)]
struct AppState {
    state: Arc<RwLock<Journeys>>,
    stops: Arc<Stops>,
    last_successful_sync: Arc<RwLock<u32>>,
    next_sync: Arc<RwLock<u32>>,
    assets_path: String,
}

#[instrument(name = "replace_state", skip_all)]
fn replace_state(siri: anyhow::Result<SiriETResponse>, state: AppState) -> anyhow::Result<()> {
    // PoisonError can _only_ happen when a thread panics while holding an exclusive lock.
    // this fn is the only place that takes this exclusive lock, and only to swap the content of it.
    // If that happens, I don't have a better idea than panicing anyway, other than maybe try to shut
    // down the whole process.
    let version = *state.next_sync.read().unwrap();
    let new_journeys = Journeys::new(&state.stops.clone(), siri?.journeys());
    let updated = new_journeys.len();
    // PoisonError again, which we can't handle.
    // We clone to avoid holding a write-lock for any operations other than swapping
    // the state out. This way, we can update `old_journeys`, then just move it into the state as
    // soon as nobody is reading it anymore. Scope to ensure we drop the lock immediately after cloning.
    let mut old_journeys = { state.state.read().unwrap().clone() };
    let old = old_journeys.len();
    let cutoff = Utc::now()
        .with_timezone(&Oslo)
        .sub(Duration::hours(8))
        .fixed_offset();
    old_journeys.expire(cutoff);
    let expired = old - old_journeys.len();
    old_journeys.merge_from(new_journeys);
    let resulting = old_journeys.len();

    // Scope to drop the lock immediately after swapping
    {
        // PoisonError. Immediately get rid of this lock.
        *state.state.write().unwrap() = old_journeys;
    }
    // We synced successfully, let's tell the health check
    {
        *state.last_successful_sync.write().unwrap() = version
    }
    {
        *state.next_sync.write().unwrap() += 1
    }
    info!("had={old} updated={updated} expired={expired} resulting={resulting} journeys.");
    Ok(())
}

fn set_up_fetch_job(
    fetch_interval_seconds: Option<u16>,
    recv_shutdown: Receiver<bool>,
    entur_config: Config,
    state: AppState,
) -> Option<tokio::task::JoinHandle<()>> {
    if let Some(interval_seconds) = fetch_interval_seconds {
        let mut recv_shutdown = recv_shutdown.clone();
        Some(tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(time::Duration::from_secs(interval_seconds as u64));

            let mut first = true;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if first {
                            first = false;
                            continue;
                        }
                        let r = replace_state(
                            entur_data::fetch_data(&entur_config).await,
                            state.clone()
                        );

                        if let Err(reason) = r {
                            error!("Unable to replace state: {reason:?}");
                        } else {
                            info!("Replaced state successfully");
                        }
                    }
                    _ = recv_shutdown.changed() => {
                        if *recv_shutdown.borrow() {
                            info!("Shutdown job");
                            break;
                        }
                    }
                }
            }
        }))
    } else {
        None
    }
}

#[derive(Serialize)]
struct Healthy {
    last_successful_sync: Option<u32>,
    next_sync_attempt: Option<u32>,
    healthy: bool,
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

async fn healthy(State(app_state): State<AppState>) -> Healthy {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_span_events(FmtSpan::CLOSE),
        )
        .with(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Forsinka::try_parse()?;
    let Commands::Serve {
        shared_options,
        port,
        fetch_interval_seconds,
        assets_path,
    } = args.command;

    let (db, data, entur_config) = initial_import(shared_options).await?;
    let stops = db::read_stops(&db)?;
    let stops = Stops::new(stops);
    let journeys = Journeys::new(&stops, data.journeys());

    let state = AppState {
        state: Arc::new(RwLock::new(journeys)),
        last_successful_sync: Arc::new(RwLock::new(0)),
        next_sync: Arc::new(RwLock::new(0)),
        stops: Arc::new(stops),
        assets_path,
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/healthy", get(healthy))
        .route("/stop/{stop_name}", get(by_stop_name))
        .route("/stops", get(stop_names))
        .route("/trains", get(train_journeys))
        .route("/trains.html", get(train_journeys_html))
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
        .with_state(state.clone());

    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
    let (send_shutdown, recv_shutdown) = tokio::sync::watch::channel(false);
    let maybe_task = set_up_fetch_job(fetch_interval_seconds, recv_shutdown, entur_config, state);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(send_shutdown))
        .await?;

    if let Some(task) = maybe_task {
        task.await?;
    }

    info!("Terminating");
    Ok(())
}
