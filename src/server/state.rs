// Application state management and background jobs
use crate::db;
use crate::entur_data::{self, Config};
use crate::entur_siriformat::SiriETResponse;
use crate::membased::{Journeys, Stops};
use chrono::{Duration, Utc};
use chrono_tz::Europe::Oslo;
use duckdb::Connection;
use reqwest::ClientBuilder;
use std::ops::Sub;
use std::sync::{Arc, RwLock};
use std::time;
use tokio::sync::watch::Receiver;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub state: Arc<RwLock<Journeys>>,
    pub stops: Arc<Stops>,
    pub last_successful_sync: Arc<RwLock<u32>>,
    pub next_sync: Arc<RwLock<u32>>,
    pub assets_path: String,
}

pub async fn initial_import(
    requestor_id: Option<String>,
    api_url: String,
    static_data: Option<String>,
    db_url: &Option<String>,
    parquet_root: &str,
    threads: u8,
    memory_gb: u8,
) -> anyhow::Result<(Connection, SiriETResponse, Config)> {
    let me = requestor_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    let client = ClientBuilder::default()
        .connect_timeout(time::Duration::from_millis(1_000))
        .timeout(time::Duration::from_millis(60_000))
        .build()?;

    let db = db::prepare_db(db_url, parquet_root, threads, memory_gb)?;
    let config = Config::new(me, api_url, client, static_data);

    let data = entur_data::fetch_data(&config).await?;
    Ok((db, data, config))
}

#[tracing::instrument(name = "replace_state", skip_all)]
pub fn replace_state(siri: anyhow::Result<SiriETResponse>, state: AppState) -> anyhow::Result<()> {
    // PoisonError can _only_ happen when a thread panics while holding an exclusive lock.
    // this fn is the only place that takes this exclusive lock, and only to swap the content of it.
    // If that happens, I don't have a better idea than panicing anyway, other than maybe try to shut
    // down the whole process.
    let version = *state.next_sync.read().unwrap();
    let new_journeys = Journeys::new(&state.stops, siri?.journeys());
    let updated = new_journeys.len();
    // PoisonError again, which we can't handle.
    // We clone to avoid holding a write-lock for any operations other than swapping
    // the state out. This way, we can update `old_journeys`, then just move it into the state as
    // soon as nobody is reading it anymore. Scope to ensure we drop the lock immediately after cloning.
    let mut old_journeys = { state.state.read().unwrap().clone() };
    let old = old_journeys.len();
    let cutoff = Utc::now()
        .with_timezone(&Oslo)
        .sub(Duration::hours(1))
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

pub fn set_up_fetch_job(
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
