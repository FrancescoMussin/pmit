use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub polymarket_ws_url: String,
    pub polymarket_data_api_url: String,
    pub poll_interval_secs: u64,
    pub large_trade_threshold: f64,
    pub markets: Vec<String>,
}

impl Config {
    /// Loads the configuration from the `.env` file and environment variables.
    pub fn load() -> Result<Self> {
        // Load variables from .env if it exists
        dotenvy::dotenv().ok();

        let polymarket_ws_url = env::var("POLYMARKET_WS_URL")
            .unwrap_or_else(|_| "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string());

        let polymarket_data_api_url = env::var("POLYMARKET_DATA_API_URL")
            .unwrap_or_else(|_| "https://data-api.polymarket.com".to_string());

        let poll_interval_secs: u64 = env::var("POLL_INTERVAL_SECS")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .context("POLL_INTERVAL_SECS must be a valid number")?;

        let large_trade_threshold: f64 = env::var("LARGE_TRADE_THRESHOLD")
            .unwrap_or_else(|_| "1000.0".to_string())
            .parse()
            .context("LARGE_TRADE_THRESHOLD must be a valid number")?;

        let markets_str = env::var("MARKETS")
            .context("MARKETS environment variable must be set (comma separated list of token IDs). Example: MARKETS=161168,161170")?;
        
        let markets: Vec<String> = markets_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if markets.is_empty() {
            anyhow::bail!("MARKETS environment variable is empty. Please provide at least one market ID.");
        }

        Ok(Config {
            polymarket_ws_url,
            polymarket_data_api_url,
            poll_interval_secs,
            large_trade_threshold,
            markets,
        })
    }
}
