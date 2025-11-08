use crate::cli::{Commands, Forsinka};
use crate::membased::{Journeys, Stops};
use crate::server::infra;
use crate::server::state::{self, AppState};
use clap::Parser;
use std::sync::{Arc, RwLock};
use tracing::info;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{filter::EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod cli;
mod db;
mod entur_data;
mod entur_siriformat;
mod handlers;
mod membased;
mod routes;
mod server;

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

    let (db, data, entur_config) = state::initial_import(
        shared_options.requestor_id,
        shared_options.api_url,
        shared_options.static_data,
        &shared_options.db_url,
        &shared_options.parquet_root,
        shared_options.threads,
        shared_options.memory_gb,
    )
    .await?;
    let stops = db::read_stops(&db)?;
    let stops = Stops::new(stops);
    let journeys = Journeys::new(&stops, data.journeys());

    let app_state = AppState {
        state: Arc::new(RwLock::new(journeys)),
        last_successful_sync: Arc::new(RwLock::new(0)),
        next_sync: Arc::new(RwLock::new(0)),
        stops: Arc::new(stops),
        assets_path,
    };

    let app = routes::create_router(app_state.clone());

    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(addr.as_str()).await?;
    let (send_shutdown, recv_shutdown) = tokio::sync::watch::channel(false);
    let maybe_task = state::set_up_fetch_job(
        fetch_interval_seconds,
        recv_shutdown,
        entur_config,
        app_state,
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(infra::shutdown_signal(send_shutdown))
        .await?;

    if let Some(task) = maybe_task {
        task.await?;
    }

    info!("Terminating");
    Ok(())
}
