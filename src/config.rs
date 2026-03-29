use anyhow::{Context, Result};
use std::env;

/// Struct to hold the application configuration loaded from the .env file and environment
/// variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// The base URL for the Polymarket Data API.
    pub polymarket_data_api_url: String,
    /// The interval in seconds at which the application polls the API for new data.
    pub poll_interval_secs: u64,
    /// The maximum number of recent trades to fetch globally across all markets.
    pub global_trades_limit: usize,
    /// The threshold in USD for considering a trade as "large".
    pub large_trade_threshold: f64,
    /// Exposure score threshold used by routing after exposure scoring.
    pub exposure_threshold: f64,
    /// Softmax temperature used by the sentence-BERT exposure scorer.
    pub exposure_temperature: f64,
    /// The number of seconds after which to warn about a stale feed.
    pub stale_feed_warn_secs: u64,
    /// The number of consecutive polls after which to consider the feed stale.
    pub stale_feed_consecutive_polls: u32,
    pub trades_db_path: String,
    pub user_history_db_path: String,
    pub training_db_path: String,
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

        let exposure_threshold: f64 = env::var("EXPOSURE_THRESHOLD")
            .unwrap_or_else(|_| "0.60".to_string())
            .parse()
            .context("EXPOSURE_THRESHOLD must be a valid number")?;

        let exposure_temperature: f64 = env::var("EXPOSURE_TEMPERATURE")
            .unwrap_or_else(|_| "0.30".to_string())
            .parse()
            .context("EXPOSURE_TEMPERATURE must be a valid number")?;

        if exposure_temperature <= 0.0 {
            anyhow::bail!("EXPOSURE_TEMPERATURE must be > 0");
        }

        let stale_feed_warn_secs: u64 = env::var("STALE_FEED_WARN_SECS")
            .unwrap_or_else(|_| "90".to_string())
            .parse()
            .context("STALE_FEED_WARN_SECS must be a valid number")?;

        let stale_feed_consecutive_polls: u32 = env::var("STALE_FEED_CONSECUTIVE_POLLS")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .context("STALE_FEED_CONSECUTIVE_POLLS must be a valid number")?;

        let trades_db_path = env::var("TRADES_DB_PATH")
            .or_else(|_| env::var("SQLITE_DB_PATH"))
            .unwrap_or_else(|_| "./databases/trades.db".to_string());

        let user_history_db_path = env::var("USER_HISTORY_DB_PATH")
            .unwrap_or_else(|_| "./databases/user_history.db".to_string());

        let training_db_path =
            env::var("TRAINING_DB_PATH").unwrap_or_else(|_| "./databases/training.db".to_string());

        Ok(Config {
            polymarket_data_api_url,
            poll_interval_secs,
            global_trades_limit,
            large_trade_threshold,
            exposure_threshold,
            exposure_temperature,
            stale_feed_warn_secs,
            stale_feed_consecutive_polls,
            trades_db_path,
            user_history_db_path,
            training_db_path,
        })
    }
}
