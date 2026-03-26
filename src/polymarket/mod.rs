use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod ws;

/// Struct representing a trade on Polymarket, with fields corresponding to the JSON structure returned by the Polymarket Data API.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Trade {
    // this makes it so that when serde converts this to json (and viceversa), tha maker_adress is
    // named as proxyWallet, which is how it is in the API response.
    #[serde(alias = "proxyWallet")]
    pub maker_address: String,
    pub side: String,
    pub asset: String,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub size: f64,
    pub price: f64,
    pub timestamp: u64,
    // same thing here
    #[serde(alias = "transactionHash")]
    pub transaction_hash: String,
}

/// Fetch global trades from the Polymarket Data API
pub async fn fetch_global_trades(
    client: &reqwest::Client,
    data_api_url: &str,
    limit: usize,
) -> Result<Vec<Trade>> {
    // this block does one full API request cycle and turns the response into a vector of Trade structs.

    // Add a timestamp cache buster to bypass Cloudflare CDN caching (meaning we get the most recent trades
    // instead of cached ones) and ensure we are always getting the latest data.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    // we construct the URL for the API request, including the limit and the timestamp cache buster.
    let url = format!("{}/trades?limit={}&_t={}", data_api_url, limit, ts);
    // we send the GET request to the API and wait for the response.
    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to send GET request for global trades")?
        .error_for_status()
        .context("Polymarket API returned an error for global trades")?;
    // we parse the JSON response into a vector of Trade structs.
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
    // we get the url out of which we have to fetch the user activity
    let url = format!("{}/activity?user={}", data_api_url, user_address);

    // we send the GET request to the API and wait for the response.
    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to send GET request for user activity")?
        .error_for_status()
        .context("Polymarket API returned an error for user activity")?;

    // we parse this into a vec of Values because the structure of the user activity
    // response is more complex and can contain different types of activities, so we
    // will handle the parsing of these activities later on when we process the user activity data.
    let activity = res
        .json::<Vec<Value>>()
        .await
        .context("Failed to parse JSON response for user activity")?;

    Ok(activity)
}
