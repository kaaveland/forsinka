use crate::entur_data::append_data;
use clap::Parser;
use reqwest;
use reqwest::Client;
use tracing::info;
use tracing_subscriber::fmt;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod db;
mod entur_data;
mod entur_siriformat;

#[derive(Parser)]
#[command(name = "forsinka")]
#[command(about = "Run API to expose current public transit delays in Norway")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Forsinka {
    /// Instead of connecting to Entur API, use a static json data file that has been downloaded
    #[arg(short = 's', long = "static-data")]
    static_data: Option<String>,
    /// Host the webapp on this particular port
    #[arg(short = 'p', long = "port", default_value = "4500")]
    port: u16,
    /// Retrieve the data from an alternate source. The source must deliver json-data that matches
    /// the Entur API.
    #[arg(short = 'u', long = "api-url", default_value = entur_data::ENTUR_API_URL)]
    api_url: String,
    /// Check for new data every fetch-interval seconds. If not provided, never refetch.
    #[arg(short = 'i', long = "fetch-interval-seconds")]
    fetch_interval_seconds: Option<u16>,
    /// DuckDB to connect to - uses an inmemory-db if not configured.
    #[arg(short = 'd', long = "db-url")]
    db_url: Option<String>,
    /// URL or file path to fetch quays.parquet and stops.parquet for geolocating stops
    #[arg(
        long = "parquet-root",
        default_value = "https://kaaveland-bus-eta-data.hel1.your-objectstorage.com/"
    )]
    parquet_root: String,
    /// requestorId to send to entur api, to fetch only diff since last fetch
    /// The default behaviour is to generate a unique one startup to receive the full dataset the first run
    #[arg(long = "requestor-id")]
    requestor_id: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Forsinka::try_parse()?;
    let me = args
        .requestor_id
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

    Ok(())
}
