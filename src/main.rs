use clap::Parser;

mod entur;
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
    #[arg(short = 'u', long = "api-url", default_value = entur::ENTUR_API_URL)]
    api_url: String,
    /// Check for new data every fetch-interval seconds. If not provided, never refetch.
    #[arg(short = 'i', long = "fetch-interval-seconds")]
    fetch_interval_seconds: Option<u16>,
    /// DuckDB to connect to - uses an inmemory-db if not configured.
    #[arg(short = 'd', long = "db-url")]
    db_url: Option<String>
}


fn main() -> anyhow::Result<()> {
    let args = Forsinka::parse();
    Ok(())
}
