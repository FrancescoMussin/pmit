use anyhow::{Context, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub polymarket_data_api_url: String,
    pub poll_interval_secs: u64,
    pub global_trades_limit: usize,
    pub large_trade_threshold: f64,
    pub stale_feed_warn_secs: u64,
    pub stale_feed_consecutive_polls: u32,
    pub sqlite_db_path: String,
}

impl Config {
    /// Loads the configuration from the `.env` file and environment variables.
    pub fn load() -> Result<Self> {
        // Load variables from .env if it exists
        dotenvy::dotenv().ok();

        let polymarket_data_api_url = env::var("POLYMARKET_DATA_API_URL")
            .unwrap_or_else(|_| "https://data-api.polymarket.com".to_string());

        let poll_interval_secs: u64 = env::var("POLL_INTERVAL_SECS")
            .unwrap_or_else(|_| "10".to_string())
            .parse()
            .context("POLL_INTERVAL_SECS must be a valid number")?;

        let global_trades_limit: usize = env::var("GLOBAL_TRADES_LIMIT")
            .unwrap_or_else(|_| "1000".to_string())
            .parse()
            .context("GLOBAL_TRADES_LIMIT must be a valid number")?;

        let large_trade_threshold: f64 = env::var("LARGE_TRADE_THRESHOLD")
            .unwrap_or_else(|_| "1000.0".to_string())
            .parse()
            .context("LARGE_TRADE_THRESHOLD must be a valid number")?;

        let stale_feed_warn_secs: u64 = env::var("STALE_FEED_WARN_SECS")
            .unwrap_or_else(|_| "90".to_string())
            .parse()
            .context("STALE_FEED_WARN_SECS must be a valid number")?;

        let stale_feed_consecutive_polls: u32 = env::var("STALE_FEED_CONSECUTIVE_POLLS")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .context("STALE_FEED_CONSECUTIVE_POLLS must be a valid number")?;

        let sqlite_db_path = env::var("SQLITE_DB_PATH").unwrap_or_else(|_| "./pmit.db".to_string());

        Ok(Config {
            polymarket_data_api_url,
            poll_interval_secs,
            global_trades_limit,
            large_trade_threshold,
            stale_feed_warn_secs,
            stale_feed_consecutive_polls,
            sqlite_db_path,
        })
    }
}
