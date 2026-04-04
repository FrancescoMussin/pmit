use anyhow::{Context, Result};
use crate::data_structures::Event;

#[derive(Debug, Clone)]
pub struct PolymarketGammaApi {
    client: reqwest::Client,
    base_url: String,
}

impl PolymarketGammaApi {
    pub fn new(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    /// Fetch active events with their underlying markets
    pub async fn fetch_active_events(&self, limit: usize) -> Result<Vec<Event>> {
        let url = format!("{}/events?limit={}&active=true&order=volume24hr&ascending=false", self.base_url, limit);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for active events")?
            .error_for_status()
            .context("Polymarket Gamma API returned an error for active events")?;

        let events = res
            .json::<Vec<Event>>()
            .await
            .context("Failed to parse JSON response for active events")?;

        Ok(events)
    }
}
