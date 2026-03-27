use anyhow::{Context, Result};
use serde_json::Value;

pub use crate::data_structures::trade::Trade;

/// HTTP adapter for Polymarket Data API.
///
/// Intended flow for the refactor:
/// - call `fetch_global_trades_raw_json` here
/// - hand raw payload to `TradeIngestor`
/// - let the ingestor normalize into stage-friendly structs
#[derive(Debug, Clone)]
pub struct PolymarketDataApi {
    client: reqwest::Client,
    base_url: String,
}

impl PolymarketDataApi {
    /// Create a new instance of the PolymarketDataApi with the given HTTP client and base URL.
    // the reqwest client is initialized in the main, the base url is passed from the config
    pub fn new(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    /// This function fetches the global trades from the Polymarket Data API and returns the raw JSON payload as a `Value`.
    pub async fn fetch_global_trades_raw_json(&self, limit: usize) -> Result<Value> {
        let url = self.global_trades_url(limit);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for global trades")?
            .error_for_status()
            .context("Polymarket API returned an error for global trades")?;

        let payload = res
            .json::<Value>()
            .await
            .context("Failed to parse JSON response for global trades")?;

        if !payload.is_array() {
            anyhow::bail!("Expected global trades payload to be a JSON array");
        }

        Ok(payload)
    }
    
    /// Fetch and decode global trades directly into typed `Trade` records.
    pub async fn fetch_global_trades(&self, limit: usize) -> Result<Vec<Trade>> {
        let payload = self.fetch_global_trades_raw_json(limit).await?;
        let trades = serde_json::from_value::<Vec<Trade>>(payload)
            .context("Failed to decode global trades into Trade structs")?;
        Ok(trades)
    }

    /// Fetch a specific user's betting history from the Polymarket Data API.
    pub async fn fetch_user_activity(&self, user_address: &str) -> Result<Vec<Value>> {
        let url = format!("{}/activity?user={}", self.base_url, user_address);

        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for user activity")?
            .error_for_status()
            .context("Polymarket API returned an error for user activity")?;

        let activity = res
            .json::<Vec<Value>>()
            .await
            .context("Failed to parse JSON response for user activity")?;

        Ok(activity)
    }

    fn global_trades_url(&self, limit: usize) -> String {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("{}/trades?limit={}&_t={}", self.base_url, limit, ts)
    }
}

/// Fetch global trades from the Polymarket Data API
pub async fn fetch_global_trades(
    client: &reqwest::Client,
    data_api_url: &str,
    limit: usize,
) -> Result<Vec<Trade>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_global_trades(limit).await
}

/// Fetch raw global trades JSON payload from the Polymarket Data API.
pub async fn fetch_global_trades_raw_json(
    client: &reqwest::Client,
    data_api_url: &str,
    limit: usize,
) -> Result<Value> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_global_trades_raw_json(limit).await
}

/// Fetch a specific user's betting history from the Polymarket Data API
pub async fn fetch_user_activity(
    client: &reqwest::Client,
    data_api_url: &str,
    user_address: &str,
) -> Result<Vec<Value>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_user_activity(user_address).await
}
