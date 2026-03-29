#![allow(warnings)]

use anyhow::{Context, Result};
use lru::LruCache;
use pmit::config::Config;
use pmit::data_structures::WalletAddress;
use pmit::database_handler::{
    TradeDatabaseHandler, UserHistoryDatabaseHandler, shutdown_database_cleanly,
    ensure_database_file,
};
use pmit::exposure::ExposureEngine;
use pmit::ingestor::TradeIngestor;
use pmit::investigator::UserActivityProfiler;
use pmit::polymarket::{self, Trade};
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};

use tracing_subscriber::EnvFilter;

// we return a Result from main so we can use the `?` operator for easier error handling
#[tokio::main]
async fn main() -> Result<()> {
    // set up the tracing_subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(tracing::Level::INFO.into())
                .from_env_lossy(),
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
    let user_history_db =
        UserHistoryDatabaseHandler::new(config.user_history_db_path.clone()).await?;
    user_history_db.init_schema().await?;

    // Training data is managed by Python workflows; we only ensure the DB file exists.
    ensure_database_file(&config.training_db_path).await?;

    tracing::info!(
        "Loaded config! Polling Global Trades API every {} seconds (limit={})",
        config.poll_interval_secs,
        config.global_trades_limit
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
        config.stale_feed_warn_secs,
        config.stale_feed_consecutive_polls
    );
    // 3. Set up our LRU (Least Recently Used) Caches
    // This cache limits memory usage by only remembering the 1,000 most recently queried addresses
    // the value here is () because only membership in the cache matters, not the value itself
    let mut recent_users: LruCache<WalletAddress, ()> = LruCache::new(
        NonZeroUsize::new(1000).context("Invalid nonzero size for recent users LRU cache")?,
    );

    // We also need a cache to remember which trades we've already processed,
    // so our 10-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> = LruCache::new(
        NonZeroUsize::new(5000).context("Invalid nonzero size for processed trades LRU cache")?,
    );

    // 4. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    // we initialize the trade ingestor, which is gonna eat up all trades returned by the API and
    // normalize them into our internal Trade struct, while also persisting the raw JSON and
    // normalized data to our trades database
    let trade_ingestor = TradeIngestor::new();
    // we also asynchronously initialize the exposure engine, which will spawn a Python worker
    // process running the sentence-BERT model for scoring trade exposure, and set up the
    // communication channels for sending trade data and receiving exposure scores.
    let mut exposure_engine = ExposureEngine::new(config.exposure_temperature).await?;
    // finally we initialize the user activity profiler, which handles routed trades and
    // fetches/persists user activity snapshots for high-value maker profiles.
    let user_activity_profiler = UserActivityProfiler::new();

    // 5. Initialize the Processing Channel
    // We use a bounded channel of 100 batches to provide backpressure if the scoring/profiling
    // falls too far behind the polling.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<Trade>>(100);

    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 6. Spawn the Background Processing Task
    // This task owns the heavy-duty components: the ExposureEngine (ML), the UserActivityProfiler,
    // and the LRU caches for deduplication and recent users.
    let config_clone = config.clone();
    let http_client_clone = http_client.clone();
    let user_history_db_clone = user_history_db.clone();

    let processing_handle = tokio::spawn(async move {
        let mut exposure_engine = exposure_engine;
        let mut processed_trades = processed_trades;
        let mut recent_users = recent_users;
        let mut stale_streak: u32 = 0;

        tracing::info!("Background processing task started.");

        // we wait for the first batch of trades to arrive
        while let Some(trades) = rx.recv().await {
            // First we need to check if the feed is stale.
            // We do this in the processing task because it's part of the analysis pipeline.
            stale_streak = update_stale_feed_streak(
                &trades,
                stale_streak,
                config_clone.stale_feed_warn_secs,
                config_clone.stale_feed_consecutive_polls,
            );

            // Deduplicate: only process trades we haven't seen in the LRU cache yet.
            let new_trades = extract_unseen_trades(trades, &mut processed_trades);
            if new_trades.is_empty() {
                continue;
            }

            // Feed the trades to the Exposure engine (ML Scoring)
            let scored_trades = match exposure_engine.score_batch(new_trades).await {
                Ok(scored) => scored,
                Err(e) => {
                    tracing::error!("Exposure scoring failed for this batch: {:?}", e);
                    continue;
                }
            };

            // Exposure -> Routing (temporary routing policy)
            let exposure_threshold = config_clone.exposure_threshold;
            let (relevant_trades, deferred_trades) =
                pmit::exposure::split_by_threshold(scored_trades, exposure_threshold);

            tracing::info!(
                "Exposure routing: relevant={}, deferred={} (threshold={:.2})",
                relevant_trades.len(),
                deferred_trades.len(),
                exposure_threshold
            );

            // Routing -> UserActivityProfiler (History Fetching)
            user_activity_profiler.profile_batch(
                relevant_trades,
                &mut recent_users,
                &http_client_clone,
                &user_history_db_clone,
                &config_clone,
            );
        }

        tracing::info!("Background processing task shutting down.");
    });

    // 7. Background Polling Loop
    tracing::info!(
        "Starting global trade monitor... Polling every {}s",
        config.poll_interval_secs
    );

    // We start the polling loop, which will run indefinitely until the program is stopped
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Shutdown signal received. Cleaning up...");
                break;
            }
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
                    Ok(raw_payload) => {
                        // We pass the raw payload to the ingestor, which will parse it, normalize it,
                        // and also persist the trades to our trades.db.
                        let ingested_batch = match trade_ingestor.ingest_raw_value(raw_payload, &trades_db).await
                        {
                            Ok(batch) => batch,
                            Err(e) => {
                                tracing::error!("Error ingesting raw trades payload: {:?}", e);
                                continue;
                            }
                        };

                        let trades = ingested_batch.into_trades();
                        if trades.is_empty() {
                            tracing::debug!(
                                "    (No trades returned in the last {} seconds)",
                                config.poll_interval_secs
                            );
                            continue;
                        }

                        // Offload the heavy processing to the background task.
                        if let Err(e) = tx.try_send(trades) {
                            tracing::warn!("Processing channel full or closed! Batch dropped: {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error fetching global trades: {:?}", e);
                    }
                }
            }
        }
    }

    // Graceful Shutdown Sequence:
    // 1. Drop the sender so the receiver task knows no more data is coming.
    drop(tx);

    // 2. Wait for the processing task to finish its final batch.
    let _ = processing_handle.await;

    // Before shutting down, we want to switch the database to DELETE mode,
    // which flushes all WAL data and removes the temporary WAL files.
    if let Err(e) = trades_db.shutdown_cleanly().await {
        tracing::error!("Failed to clean shutdown trades DB: {:?}", e);
    }

    if let Err(e) = user_history_db.shutdown_cleanly().await {
        tracing::error!("Failed to clean shutdown user history DB: {:?}", e);
    }

    if let Err(e) = shutdown_database_cleanly(&config.training_db_path).await {
        tracing::error!("Failed to clean shutdown training DB: {:?}", e);
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
            lag_secs,
            stale_feed_warn_secs,
            next_streak
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
                current_streak,
                lag_secs
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
