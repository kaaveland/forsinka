use crate::api::JourneyDelay;
use crate::api::TrainJourney;
use crate::api::TrainsPage;
use crate::entur_data::{Config, vehicle_journeys};
use anyhow::anyhow;
use askama::Template;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router, http};
use clap::{Parser, Subcommand};
use duckdb::Connection;
use http::HeaderValue;
use http::header::CACHE_CONTROL;
use reqwest::ClientBuilder;
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;
use tokio::signal;
use tokio::sync::watch::{Receiver, Sender};
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tower_http::cors;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{error, info};
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod api;
mod db;
mod entur_data;
mod entur_siriformat;

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
    /// Create a local DuckDB file with imported data from entur
    ///
    /// This is convenient to explore the data
    Import {
        #[command(flatten)]
        shared_options: SharedOptions,
    },
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

async fn initial_import(args: SharedOptions) -> anyhow::Result<(Connection, Config)> {
    let me = args
        .requestor_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let client = ClientBuilder::default()
        .connect_timeout(Duration::from_millis(1_000))
        .timeout(Duration::from_millis(60_000))
        .build()?;

    let mut db = db::prepare_db(
        &args.db_url,
        &args.parquet_root,
        args.threads,
        args.memory_gb,
    )?;
    let config = Config::new(me, args.api_url.clone(), client, args.static_data);

    let data = entur_data::fetch_data(&config).await?;
    let vehicle_journeys = entur_data::vehicle_journeys(data, 0);
    info!("Start inserting journeys");
    db::replace_data(&mut db, vehicle_journeys)?;
    info!("Loaded initial data");
    Ok((db, config))
}

async fn root() -> &'static str {
    "Hello, World"
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

async fn refetch_data(
    db: &mut Connection,
    entur_config: &Config,
    version: u32,
) -> anyhow::Result<()> {
    let data = entur_data::fetch_data(entur_config).await?;
    let journeys = vehicle_journeys(data, version);
    db::replace_data(db, journeys)?;
    Ok(())
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

async fn by_stop_name(
    State(state): State<AppState>,
    Path(stop_name): Path<String>,
) -> Result<Json<Vec<JourneyDelay>>, WebappError> {
    let c = state.conn()?;
    let v = api::journey_delays(stop_name.as_str(), &c)?;
    Ok(Json(v))
}

async fn train_journeys(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrainJourney>>, WebappError> {
    let c = state.conn()?;
    let v = api::train_journeys(&c)?;
    Ok(Json(v))
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

async fn train_journeys_html(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, WebappError> {
    let c = state.conn()?;
    let trains = api::train_journeys(&c)?;
    Ok(TrainsPage::new(trains, state.assets_path.clone()))
}

async fn stop_names(State(state): State<AppState>) -> Result<Json<Vec<String>>, WebappError> {
    let c = state.conn()?;
    let stops: Result<Vec<String>, _> = c.
        prepare("from stopdata join estimated_call using (stop_point_ref) select distinct name where name is not null")?.
        query_map([], |row| row.get(0))?.collect();
    Ok(Json(stops?))
}

fn set_up_fetch_job(
    fetch_interval_seconds: Option<u16>,
    recv_shutdown: Receiver<bool>,
    mut db: Connection,
    entur_config: Config,
    state: AppState,
) -> Option<tokio::task::JoinHandle<()>> {
    if let Some(interval_seconds) = fetch_interval_seconds {
        let mut recv_shutdown = recv_shutdown.clone();
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds as u64));

            let mut first = true;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if first {
                            first = false;
                            continue;
                        }
                        let version = *state.next_sync.read().unwrap();
                        if let Err(reason) = refetch_data(&mut db, &entur_config, version).await {
                            error!("Unable to refetch: {reason:?}");
                        } else if let Ok(mut sync_version) = state.last_successful_sync.write() {
                                *sync_version = version;
                        }
                        if let Ok(mut sync_version) = state.next_sync.write() {
                            *sync_version += 1;
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

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    last_successful_sync: Arc<RwLock<u32>>,
    next_sync: Arc<RwLock<u32>>,
    assets_path: String,
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

impl AppState {
    fn conn(&self) -> Result<MutexGuard<'_, Connection>, anyhow::Error> {
        self.db
            .lock()
            .map_err(|err| anyhow!("Failed to get db: {err}"))
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

    match args.command {
        Commands::Import { shared_options } => {
            initial_import(shared_options).await?;
            Ok(())
        }
        Commands::Serve {
            shared_options,
            port,
            fetch_interval_seconds,
            assets_path,
        } => {
            let (db, entur_config) = initial_import(shared_options).await?;
            let state = AppState {
                db: Arc::new(Mutex::new(db.try_clone()?)),
                last_successful_sync: Arc::new(RwLock::new(0)),
                next_sync: Arc::new(RwLock::new(0)),
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
                        .layer(TimeoutLayer::new(Duration::from_millis(500)))
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
            let maybe_task = set_up_fetch_job(
                fetch_interval_seconds,
                recv_shutdown,
                db,
                entur_config,
                state,
            );

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal(send_shutdown))
                .await?;

            if let Some(task) = maybe_task {
                task.await?;
            }

            info!("Terminating");
            Ok(())
        }
    }
}
