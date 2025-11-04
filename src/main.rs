use crate::api::JourneyDelay;
use crate::entur_data::{Config, vehicle_journeys};
use anyhow::anyhow;
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
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::signal;
use tokio::sync::watch::Sender;
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tower_http::cors;
use tower_http::cors::CorsLayer;
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
            terminate_jobs.send(true).expect("Unable to terminate jobs");
        },
        _ = terminate => {
            info!("Received terminate signal");
            terminate_jobs.send(true).expect("Unable to terminate jobs");
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
    State(conn): State<Arc<Mutex<Connection>>>,
    Path(stop_name): Path<String>,
) -> Result<Json<Vec<JourneyDelay>>, WebappError> {
    let c = conn
        .lock()
        .map_err(|err| anyhow!("Unable to take conn: {err:?}"))?;
    let v = api::journey_delays(stop_name.as_str(), &c)?;
    Ok(Json(v))
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
        } => {
            let (mut db, entur_config) = initial_import(shared_options).await?;
            let app_db = db.try_clone()?;

            let app = Router::new()
                .route("/", get(root))
                .route("/stop/{stop_name}", get(by_stop_name))
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
                .with_state(Arc::new(Mutex::new(app_db)));

            let addr = format!("0.0.0.0:{}", port);

            let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
            let (send_shutdown, recv_shutdown) = tokio::sync::watch::channel(false);

            let maybe_task = if let Some(interval_seconds) = fetch_interval_seconds {
                let mut recv_shutdown = recv_shutdown.clone();
                Some(tokio::spawn(async move {
                    let mut interval =
                        tokio::time::interval(Duration::from_secs(interval_seconds as u64));

                    let mut first = true;
                    let mut version = 1;

                    loop {
                        tokio::select! {
                            _ = interval.tick() => {
                                if first {
                                    first = false;
                                    continue;
                                }
                                if let Err(reason) = refetch_data(&mut db, &entur_config, version).await {
                                    error!("Unable to refetch: {reason:?}");
                                }

                            }
                            _ = recv_shutdown.changed() => {
                                if *recv_shutdown.borrow() {
                                    info!("Shutdown job");
                                    break;
                                }
                            }
                        }
                        version += 1;
                    }
                }))
            } else {
                None
            };

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
