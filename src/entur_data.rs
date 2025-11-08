use crate::entur_siriformat::SiriETResponse;
use reqwest::Client;
use std::fs;
use tracing::{info, instrument};

pub const ENTUR_API_URL: &str = "https://api.entur.io/realtime/v1/rest/et";

pub struct Config {
    requestor_id: String,
    api_url: String,
    client: Client,
    static_data: Option<String>,
}

impl Config {
    pub fn new(
        requestor_id: String,
        api_url: String,
        client: Client,
        static_data: Option<String>,
    ) -> Self {
        Self {
            requestor_id,
            api_url,
            client,
            static_data,
        }
    }
}

#[instrument(name = "fetch_siri", skip(config))]
async fn fetch_siri(config: &Config) -> anyhow::Result<SiriETResponse> {
    let url = config.api_url.as_str();
    let requestor_id = config.requestor_id.as_str();
    info!("Poll {url} with requestorId={requestor_id}");
    Ok(config
        .client
        .get(url)
        // TODO: We're getting the entire dataset each time for some reason?
        // Might not be a problem since we're pretty fast anyway.
        .query(&[("requestorId", requestor_id)])
        .header("Accept", "application/json")
        .send()
        .await?
        .json()
        .await?)
}

pub async fn fetch_data(config: &Config) -> anyhow::Result<SiriETResponse> {
    if let Some(path) = &config.static_data {
        let content = fs::read(path)?;
        Ok(serde_json::from_slice(&content)?)
    } else {
        fetch_siri(config).await
    }
}
