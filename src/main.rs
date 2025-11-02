use crate::entur_data::append_data;
use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use clap::{Parser, Subcommand};
use duckdb::Connection;
use reqwest;
use reqwest::Client;
use std::time::Duration;
use tokio::signal;
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tracing::{error, info};
use tracing_subscriber::fmt;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

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

async fn initial_import(args: &SharedOptions) -> anyhow::Result<Connection> {
    let me = args
        .requestor_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let client = Client::new();
    let db = db::prepare_db(&args.db_url, &args.parquet_root)?;

    let data =
        entur_data::fetch_initial_data(&args.static_data, &args.api_url, me.as_str(), &client)
            .await?;
    let vehicle_journeys = entur_data::vehicle_journeys(data);
    info!("Start inserting journeys");
    append_data(vehicle_journeys, &db)?;
    info!("Load initial SIRI-et data into DuckDB");
    Ok(db)
}

async fn root() -> &'static str {
    "Hello, World"
}

async fn shutdown_signal() {
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
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Forsinka::try_parse()?;

    match args.command {
        Commands::Import { shared_options } => {
            let _db = initial_import(&shared_options).await?;
            Ok(())
        }
        Commands::Serve {
            shared_options,
            port,
            fetch_interval_seconds,
        } => {
            let _db = initial_import(&shared_options).await?;
            let _copy = _db.try_clone()?;

            let app = Router::new().route("/", get(root)).layer(
                ServiceBuilder::new()
                    .layer(HandleErrorLayer::new(|_: BoxError| async {
                        error!("Timed out");
                        (StatusCode::REQUEST_TIMEOUT, "Timed out. Sorry!".to_string())
                    }))
                    .layer(TimeoutLayer::new(Duration::from_millis(500))),
            );

            let addr = format!("0.0.0.0:{}", port);

            let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            info!("Terminating");
            Ok(())
        }
    }
}
