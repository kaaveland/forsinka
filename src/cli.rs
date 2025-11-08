// CLI argument definitions
use clap::{Parser, Subcommand};

#[derive(Parser)]
pub struct SharedOptions {
    /// Instead of connecting to Entur API, use a static json data file that has been downloaded
    #[arg(short = 's', long = "static-data")]
    pub static_data: Option<String>,
    /// Retrieve the data from an alternate source. The source must deliver json-data that matches
    /// the Entur API.
    #[arg(short = 'u', long = "api-url", default_value = crate::entur_data::ENTUR_API_URL)]
    pub api_url: String,
    /// URL or file path to fetch quays.parquet and stops.parquet for geolocating stops
    #[arg(
        long = "parquet-root",
        default_value = "https://kaaveland-bus-eta-data.hel1.your-objectstorage.com/"
    )]
    pub parquet_root: String,
    /// DuckDB to connect to - uses an inmemory-db if not configured.
    #[arg(short = 'd', long = "db-url")]
    pub db_url: Option<String>,
    /// requestorId to send to entur api, to fetch only diff since last fetch
    /// The default behaviour is to generate a unique on startup to receive the full dataset the first run
    #[arg(long = "requestor-id")]
    pub requestor_id: Option<String>,
    /// Number of threads to configure DuckDB with
    #[arg(short = 'j', long = "threads", default_value = "1")]
    pub threads: u8,
    /// GB of RAM to grant DuckDB
    #[arg(short = 'm', long = "memory-gbs", default_value = "1")]
    pub memory_gb: u8,
}

#[derive(Subcommand)]
pub enum Commands {
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
pub struct Forsinka {
    #[command(subcommand)]
    pub command: Commands,
}
