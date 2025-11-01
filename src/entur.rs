use crate::entur_siriformat;

pub const ENTUR_API_URL: &str = "https://api.entur.io/realtime/v1/rest/et";

pub fn fetch_siri(
    client: &reqwest::blocking::Client,
    url: &str,
    requestor_id: &str
) -> anyhow::Result<entur_siriformat::SiriETResponse> {
    Ok(client.get(url)
        .query(&[("requestorId", requestor_id)])
        .header("Accept", "application/json")
        .send()?.json()?)
}