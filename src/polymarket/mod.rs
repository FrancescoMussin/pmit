use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// The struct representing a single trade from the global firehose
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Trade {
    #[serde(alias = "proxyWallet")]
    pub maker_address: String,
    pub side: String,
    pub asset: String,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub size: f64,
    pub price: f64,
    pub timestamp: u64,
    #[serde(alias = "transactionHash")]
    pub transaction_hash: String,
}

/// Fetch global trades from the Polymarket Data API
pub async fn fetch_global_trades(
    client: &reqwest::Client,
    data_api_url: &str,
    limit: usize,
) -> Result<Vec<Trade>> {
    // Add a timestamp cache buster to bypass Cloudflare CDN caching
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let url = format!("{}/trades?limit={}&_t={}", data_api_url, limit, ts);
    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to send GET request for global trades")?
        .error_for_status()
        .context("Polymarket API returned an error for global trades")?;

    let trades = res
        .json::<Vec<Trade>>()
        .await
        .context("Failed to parse JSON response for global trades")?;

    Ok(trades)
}

/// Fetch a specific user's betting history from the Polymarket Data API
pub async fn fetch_user_activity(
    client: &reqwest::Client,
    data_api_url: &str,
    user_address: &str,
) -> Result<Vec<Value>> {
    let url = format!("{}/activity?user={}", data_api_url, user_address);

    let res = client
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
