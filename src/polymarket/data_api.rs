use anyhow::{Context, Result};
use serde_json::Value;

pub use crate::data_structures::trade::{ClosedPosition, Trade};

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

    /// This function fetches the global trades from the Polymarket Data API and returns the raw
    /// JSON payload as a `Value`.
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

    pub async fn fetch_user_activity(&self, user_address: &str) -> Result<Vec<Value>> {
        // this shouldn't have a side=BUY
        let url = format!("{}/activity?user={}&type=trade", self.base_url, user_address);

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

    /// Fetch a specific user's closed positions from the Polymarket Data API.
    pub async fn fetch_user_closed_positions(
        &self,
        user_address: &str,
    ) -> Result<Vec<ClosedPosition>> {
        let url = format!("{}/closed-positions?user={}", self.base_url, user_address);

        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for closed positions")?
            .error_for_status()
            .context("Polymarket API returned an error for closed positions")?;

        let positions = res
            .json::<Vec<ClosedPosition>>()
            .await
            .context("Failed to parse JSON response for closed positions")?;

        Ok(positions)
    }

    /// Fetch trades for a specific market (condition ID) from the Polymarket Data API.
    pub async fn fetch_market_trades_raw_json(&self, condition_id: &str, limit: usize) -> Result<Value> {
        let url = format!("{}/trades?market={}&limit={}&side=buy", self.base_url, condition_id, limit);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for market trades")?
            .error_for_status()
            .context("Polymarket API returned an error for market trades")?;

        let payload = res
            .json::<Value>()
            .await
            .context("Failed to parse JSON response for market trades")?;

        if !payload.is_array() {
            anyhow::bail!("Expected market trades payload to be a JSON array");
        }

        Ok(payload)
    }

    /// Fetch and decode trades for a specific market directly into typed `Trade` records.
    pub async fn fetch_market_trades(&self, condition_id: &str, limit: usize) -> Result<Vec<Trade>> {
        let payload = self.fetch_market_trades_raw_json(condition_id, limit).await?;
        let trades = serde_json::from_value::<Vec<Trade>>(payload)
            .context("Failed to decode market trades into Trade structs")?;
        Ok(trades)
    }

    /// Fetch and decode trades for multiple markets (comma-separated condition IDs) from the Polymarket Data API.
    pub async fn fetch_markets_trades(&self, condition_ids: &[String], limit: usize) -> Result<Vec<Trade>> {
        let payload = self.fetch_markets_trades_raw_json(condition_ids, limit).await?;
        let trades = serde_json::from_value::<Vec<Trade>>(payload)
            .context("Failed to decode multiple markets trades into Trade structs")?;
        Ok(trades)
    }

    /// Fetch trades for multiple markets (comma-separated condition IDs) as raw JSON from the Polymarket Data API.
    pub async fn fetch_markets_trades_raw_json(&self, condition_ids: &[String], limit: usize) -> Result<Value> {
        let markets_param = condition_ids.join(",");
        let url = format!("{}/trades?market={}&limit={}&side=buy", self.base_url, markets_param, limit);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for multiple markets trades raw JSON")?
            .error_for_status()
            .context("Polymarket API returned an error for multiple markets trades raw JSON")?;

        let payload = res
            .json::<Value>()
            .await
            .context("Failed to parse JSON response for multiple markets trades raw JSON")?;

        if !payload.is_array() {
            anyhow::bail!("Expected multiple markets trades payload to be a JSON array");
        }

        Ok(payload)
    }

    /// Fetch trades for an entire event (by event ID) as raw JSON from the Polymarket Data API.
    pub async fn fetch_event_trades_raw_json(&self, event_id: &str, limit: usize) -> Result<Value> {
        let url = format!("{}/trades?eventId={}&limit={}&side=buy", self.base_url, event_id, limit);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send GET request for event trades raw JSON")?
            .error_for_status()
            .context("Polymarket API returned an error for event trades raw JSON")?;

        let payload = res
            .json::<Value>()
            .await
            .context("Failed to parse JSON response for event trades raw JSON")?;

        if !payload.is_array() {
            anyhow::bail!("Expected event trades payload to be a JSON array");
        }

        Ok(payload)
    }

    fn global_trades_url(&self, limit: usize) -> String {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("{}/trades?limit={}&_t={}&side=buy", self.base_url, limit, ts)
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

pub async fn fetch_user_activity(
    client: &reqwest::Client,
    data_api_url: &str,
    user_address: &str,
) -> Result<Vec<Value>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_user_activity(user_address).await
}

/// Fetch a specific user's closed positions from the Polymarket Data API.
pub async fn fetch_user_closed_positions(
    client: &reqwest::Client,
    data_api_url: &str,
    user_address: &str,
) -> Result<Vec<ClosedPosition>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_user_closed_positions(user_address).await
}

/// Fetch trades for a specific market from the Polymarket Data API
pub async fn fetch_market_trades(
    client: &reqwest::Client,
    data_api_url: &str,
    condition_id: &str,
    limit: usize,
) -> Result<Vec<Trade>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_market_trades(condition_id, limit).await
}

/// Fetch raw trades JSON for a specific market from the Polymarket Data API
pub async fn fetch_market_trades_raw_json(
    client: &reqwest::Client,
    data_api_url: &str,
    condition_id: &str,
    limit: usize,
) -> Result<Value> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_market_trades_raw_json(condition_id, limit).await
}

/// Fetch trades for multiple markets from the Polymarket Data API
pub async fn fetch_markets_trades(
    client: &reqwest::Client,
    data_api_url: &str,
    condition_ids: &[String],
    limit: usize,
) -> Result<Vec<Trade>> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_markets_trades(condition_ids, limit).await
}

/// Fetch raw trades JSON for multiple markets from the Polymarket Data API
pub async fn fetch_markets_trades_raw_json(
    client: &reqwest::Client,
    data_api_url: &str,
    condition_ids: &[String],
    limit: usize,
) -> Result<Value> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_markets_trades_raw_json(condition_ids, limit).await
}

/// Fetch raw trades JSON for an entire event from the Polymarket Data API.
pub async fn fetch_event_trades_raw_json(
    client: &reqwest::Client,
    data_api_url: &str,
    event_id: &str,
    limit: usize,
) -> Result<Value> {
    let api = PolymarketDataApi::new(client.clone(), data_api_url.to_string());
    api.fetch_event_trades_raw_json(event_id, limit).await
}
