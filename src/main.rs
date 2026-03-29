#![allow(warnings)]

mod config;
mod data_structures;
mod database_handler;
mod exposure;
mod ingestor;
mod investigator;
mod polymarket;

use anyhow::{Context, Result};
use config::Config;
use data_structures::WalletAddress;
use database_handler::{
    TradeDatabaseHandler, UserHistoryDatabaseHandler, checkpoint_database_file,
    ensure_database_file,
};
use exposure::ExposureEngine;
use ingestor::TradeIngestor;
use investigator::UserActivityProfiler;
use lru::LruCache;
use polymarket::Trade;
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};

use tracing_subscriber::EnvFilter;

// we return a Result from main so we can use the `?` operator for easier error handling
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy()
        )
        .init();
    // 1. We load the configuration
    let config = Config::load()?;
    // 2. Initialize all database handlers and schemas before starting the polling loop.
    let trades_db = TradeDatabaseHandler::new(config.trades_db_path.clone()).await?;
    // This will create the database file if it doesn't exist, and set up the necessary tables.
    trades_db.init_schema().await?;
    // same here for the user history database, which we use to store snapshots of user activity
    // profiles when we fetch them after large trades
    let user_history_db = UserHistoryDatabaseHandler::new(config.user_history_db_path.clone()).await?;
    user_history_db.init_schema().await?;

    // Training data is managed by Python workflows; we only ensure the DB file exists.
    ensure_database_file(&config.training_db_path).await?;

    tracing::info!(
        "Loaded config! Polling Global Trades API every {} seconds (limit={})",
        config.poll_interval_secs, config.global_trades_limit
    );
    tracing::info!("Trades DB initialized at {}", config.trades_db_path);
    tracing::info!(
        "User history DB initialized at {}",
        config.user_history_db_path
    );
    tracing::info!("Training DB initialized at {}", config.training_db_path);
    tracing::info!(
        "Exposure routing threshold set to {:.2}",
        config.exposure_threshold
    );
    tracing::info!(
        "Exposure scorer temperature set to {:.2}",
        config.exposure_temperature
    );
    tracing::info!(
        "Staleness detector: warn_lag={}s, consecutive_polls={}",
        config.stale_feed_warn_secs, config.stale_feed_consecutive_polls
    );
    // 3. Set up our LRU (Least Recently Used) Caches
    // This cache limits memory usage by only remembering the 1,000 most recently queried addresses
    // the value here is () because only membership in the cache matters, not the value itself
    let mut recent_users: LruCache<WalletAddress, ()> =
        LruCache::new(NonZeroUsize::new(1000).context("Invalid nonzero size for recent users LRU cache")?);

    // We also need a cache to remember which trades we've already processed,
    // so our 10-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(5000).context("Invalid nonzero size for processed trades LRU cache")?);

    // 4. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    // we initialize the trade ingestor, which is gonna eat up all trades returned by the API and
    // normalize them into our internal Trade struct, while also persisting the raw JSON and
    // normalized data to our trades database
    let trade_ingestor = TradeIngestor::new();
    // we also asynchronously initialize the exposure engine, which will spawn a Python worker process running the
    // sentence-BERT model for scoring trade exposure, and set up the communication channels for
    // sending trade data and receiving exposure scores.
    let mut exposure_engine = ExposureEngine::new(config.exposure_temperature).await?;
    // finally we initialize the user activity profiler, which handles routed trades and
    // fetches/persists user activity snapshots for high-value maker profiles.
    let user_activity_profiler = UserActivityProfiler::new();

    // 5. Create our polling interval
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 6. Background Polling Loop

    tracing::info!(
        "Starting global trade monitor... Polling every {}s",
        config.poll_interval_secs
    );
    // this is just for keeping track of how many consecutive times we've seen a stale feed, so we
    // can log that pattern if it emerges
    let mut stale_streak: u32 = 0;
    // We start the polling loop, which will run indefinitely until the program is stopped
    loop {
        // The `tokio::select!` macro allows us to wait on multiple asynchronous events
        // simultaneously. In this case, we wait for either a shutdown signal (Ctrl+C) or
        // the next tick of our polling interval.
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Shutdown signal received. Flushing SQLite WAL checkpoints...");
                break;
            }
            // this blocks a new iteration of the loop until the configured interval has passed since the last tick
            // (because poll_interval.tick() is a future)
            _ = poll_interval.tick() => {
                tracing::debug!("--> Polling Data API for new trades...");

                // Fetch raw global trades payload and normalize via the ingestor.
                match polymarket::fetch_global_trades_raw_json(
                    &http_client,
                    &config.polymarket_data_api_url,
                    config.global_trades_limit,
                )
                .await
                {
                    // first case: we successfully got a response from the API, and we have a raw Value payload to ingest
                    // normally the api would return a json but fetch_global_trades_raw_json already transformns this into
                    // a serde_json::Value for us, so we can just pass it to the ingestor without worrying about the HTTP
                    // details here in main
                    Ok(raw_payload) => {
                        // we pass the raw payload to the ingestor, which will parse it, normalize it into our internal Trade struct,
                        // and also persist the trades to our trades.db
                        let ingested_batch = match trade_ingestor.ingest_raw_value(raw_payload, &trades_db).await
                        {
                            // we do some error handling here
                            Ok(batch) => batch,
                            Err(e) => {
                                tracing::error!("Error ingesting raw trades payload: {:?}", e);
                                continue;
                            }
                        };
                        // after ingesting we get the trades back so that we can pass them to the exposure engine for scoring and
                        // routing
                        let trades = ingested_batch.into_trades();

                        // First we need to check if the feed is stale.
                        if trades.is_empty() {
                            tracing::debug!(
                                "    (No trades returned in the last {} seconds)",
                                config.poll_interval_secs
                            );
                            continue;
                        }

                        stale_streak = update_stale_feed_streak(
                            &trades,
                            stale_streak,
                            config.stale_feed_warn_secs,
                            config.stale_feed_consecutive_polls,
                        );

                        let new_trades = extract_unseen_trades(trades, &mut processed_trades);
                        if new_trades.is_empty() {
                            tracing::debug!(
                                "    (No new trades in the last {} seconds)",
                                config.poll_interval_secs
                            );
                            continue;
                        }

                        // Ingestor -> Exposure engine
                        let scored_trades = match exposure_engine.score_batch(new_trades).await {
                            // error handling for the exposure engine, which might fail if the Python worker
                            // process crashes or returns invalid data
                            Ok(scored) => scored,
                            Err(e) => {
                                tracing::error!("Exposure scoring failed for this batch: {:?}", e);
                                continue;
                            }
                        };

                        // Exposure -> Routing (temporary routing policy)
                        let exposure_threshold = config.exposure_threshold;
                        let (relevant_trades, deferred_trades) =
                            exposure::split_by_threshold(scored_trades, exposure_threshold);

                        tracing::info!(
                            "Exposure routing: relevant={}, deferred={} (threshold={:.2})",
                            relevant_trades.len(),
                            deferred_trades.len(),
                            exposure_threshold
                        );

                        // Routing -> UserActivityProfiler
                        user_activity_profiler.profile_batch(
                            relevant_trades,
                            &mut recent_users,
                            &http_client,
                            &user_history_db,
                            &config,
                        );
                    }
                    Err(e) => {
                        tracing::error!("Error fetching global trades: {:?}", e);
                    }
                }
            }
        }
    }

    // Before shutting down, we want to checkpoint the SQLite databases to ensure all data is
    // flushed from the WAL files to the main DB files.
    if let Err(e) = trades_db.checkpoint_truncate().await {
        tracing::error!("Failed to checkpoint trades DB: {:?}", e);
    }

    if let Err(e) = user_history_db.checkpoint_truncate().await {
        tracing::error!("Failed to checkpoint user history DB: {:?}", e);
    }

    if let Err(e) = checkpoint_database_file(&config.training_db_path).await {
        tracing::error!("Failed to checkpoint training DB: {:?}", e);
    }

    tracing::info!("Shutdown checkpoint complete.");
    Ok(())
}

// helper functions to keep main() cleaner
fn update_stale_feed_streak(
    trades: &[Trade],
    current_streak: u32,
    stale_feed_warn_secs: u64,
    stale_feed_consecutive_polls: u32,
) -> u32 {
    let Some(newest_trade_ts) = trades.iter().map(|t| t.timestamp).max() else {
        return current_streak;
    };

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let lag_secs = now_ts.saturating_sub(newest_trade_ts);

    if lag_secs >= stale_feed_warn_secs {
        let next_streak = current_streak + 1;
        tracing::warn!(
            "[STALE FEED] newest trade is {}s old (threshold={}s, streak={})",
            lag_secs, stale_feed_warn_secs, next_streak
        );

        if next_streak >= stale_feed_consecutive_polls {
            tracing::error!(
                "[STALE FEED] sustained staleness detected. This often indicates CDN/API cache windows."
            );
        }

        next_streak
    } else {
        if current_streak > 0 {
            tracing::info!(
                "Feed freshness recovered after {} stale poll(s). Current lag={}s",
                current_streak, lag_secs
            );
        }
        0
    }
}

fn extract_unseen_trades(
    trades: Vec<Trade>,
    processed_trades: &mut LruCache<String, ()>,
) -> Vec<Trade> {
    let mut unseen = Vec::new();

    for trade in trades {
        if !processed_trades.contains(&trade.transaction_hash) {
            processed_trades.put(trade.transaction_hash.clone(), ());
            unseen.push(trade);
        }
    }

    unseen
}
