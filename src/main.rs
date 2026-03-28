#![allow(warnings)]

mod config;
mod data_structures;
mod database_handler;
mod exposure;
mod ingestor;
mod polymarket;
mod trade_filter;

use anyhow::Result;
use config::Config;
use data_structures::WalletAddress;
use database_handler::{
    TradeDatabaseHandler, UserHistoryDatabaseHandler, checkpoint_database_file,
    ensure_database_file,
};
use ingestor::TradeIngestor;
use lru::LruCache;
use polymarket::Trade;
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};
use trade_filter::TradeFilter;

// we return a Result from main so we can use the `?` operator for easier error handling
#[tokio::main]
async fn main() -> Result<()> {
    // 1. We load the configuration
    let config = Config::load()?;
    // 2. Initialize all database handlers and schemas before starting the polling loop.
    let trades_db = TradeDatabaseHandler::new(config.trades_db_path.clone())?;
    trades_db.init_schema()?;

    let user_history_db = UserHistoryDatabaseHandler::new(config.user_history_db_path.clone())?;
    user_history_db.init_schema()?;

    // Training data is managed by Python workflows; we only ensure the DB file exists.
    ensure_database_file(&config.training_db_path)?;

    println!(
        "Loaded config! Polling Global Trades API every {} seconds (limit={})",
        config.poll_interval_secs, config.global_trades_limit
    );
    println!("Trades DB initialized at {}", config.trades_db_path);
    println!(
        "User history DB initialized at {}",
        config.user_history_db_path
    );
    println!("Training DB initialized at {}", config.training_db_path);
    println!(
        "Staleness detector: warn_lag={}s, consecutive_polls={}",
        config.stale_feed_warn_secs, config.stale_feed_consecutive_polls
    );
    // 3. Set up our LRU (Least Recently Used) Caches
    // This cache limits memory usage by only remembering the 1,000 most recently queried addresses
    // the value here is () because only membership in the cache matters, not the value itself
    let mut recent_users: LruCache<WalletAddress, ()> =
        LruCache::new(NonZeroUsize::new(1000).unwrap());

    // We also need a cache to remember which trades we've already processed,
    // so our 10-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(5000).unwrap());

    // 4. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    let trade_ingestor = TradeIngestor::new();
    // we can initialize the trade filter
    let trade_filter = TradeFilter::new();

    // 5. Create our polling interval
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 6. Background Polling Loop
    println!(
        "Starting global trade Watchdog... Polling every {}s",
        config.poll_interval_secs
    );
    // this is just for keeping track of how many consecutive times we've seen a stale feed, so we can log that pattern if it emerges
    let mut stale_streak: u32 = 0;
    // We start the polling loop, which will run indefinitely until the program is stopped
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Shutdown signal received. Flushing SQLite WAL checkpoints...");
                break;
            }
            // this blocks a new iteration of the loop until the configured interval has passed since the last tick
            _ = poll_interval.tick() => {
        println!("--> Polling Data API for new trades...");

        // Fetch raw global trades payload and normalize via the ingestor.
        match polymarket::fetch_global_trades_raw_json(
            &http_client,
            &config.polymarket_data_api_url,
            config.global_trades_limit,
        )
        .await
        {
            Ok(raw_payload) => {
                let ingested_batch = match trade_ingestor.ingest_raw_value(raw_payload, &trades_db)
                {
                    Ok(batch) => batch,
                    Err(e) => {
                        eprintln!("Error ingesting raw trades payload: {:?}", e);
                        continue;
                    }
                };
                let trades = ingested_batch.into_trades();

                if trades.is_empty() {
                    println!(
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
                    println!(
                        "    (No new trades in the last {} seconds)",
                        config.poll_interval_secs
                    );
                    continue;
                }

                // here the ingestor should give the trades to the exposure engine

                // here the exposure engine should give the trades to the routing engine

                // the routing engine should give the relevant trades to the context engine

                // the context engine should give the trades to the final model

                for trade in new_trades {
                    handle_trade(
                        &mut recent_users,
                        &trade_filter,
                        http_client.clone(),
                        user_history_db.clone(),
                        config.clone(),
                        trade,
                    );
                }
            }
            Err(e) => {
                eprintln!("Error fetching global trades: {:?}", e);
            }
        }
            }
        }
    }

    if let Err(e) = trades_db.checkpoint_truncate() {
        eprintln!("Failed to checkpoint trades DB: {:?}", e);
    }

    if let Err(e) = user_history_db.checkpoint_truncate() {
        eprintln!("Failed to checkpoint user history DB: {:?}", e);
    }

    if let Err(e) = checkpoint_database_file(&config.training_db_path) {
        eprintln!("Failed to checkpoint training DB: {:?}", e);
    }

    println!("Shutdown checkpoint complete.");
    Ok(())
}

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
        eprintln!(
            "[STALE FEED] newest trade is {}s old (threshold={}s, streak={})",
            lag_secs, stale_feed_warn_secs, next_streak
        );

        if next_streak >= stale_feed_consecutive_polls {
            eprintln!(
                "[STALE FEED] sustained staleness detected. This often indicates CDN/API cache windows."
            );
        }

        next_streak
    } else {
        if current_streak > 0 {
            println!(
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

/// This function processes each trade.
/// First it prints out the trade details if it passes the trade_filter, then if the trade value is above our configured threshold,
/// it checks if we've recently profiled this user. If not, it spawns a new asynchronous task to fetch their activity and persist
/// it to the database, while also printing a preview of their profile to the console.
fn handle_trade(
    recent_users: &mut LruCache<WalletAddress, ()>,
    trade_filter: &TradeFilter,
    client: reqwest::Client,
    user_history_db: UserHistoryDatabaseHandler,
    config: Config,
    trade: Trade,
) {
    let total_value = trade.size * trade.price;
    let bet_title = trade.title.as_deref().unwrap_or("Unknown Market");
    let bet_outcome = trade.outcome.as_deref().unwrap_or("N/A");
    let should_print_trade = trade_filter.should_print_trade(&trade);

    // we print all trades that pass the filter
    if should_print_trade {
        println!(
            "🚨 TRADE: {} [{}] | Share Size: {:.2} @ Price: ${:.2} (Value: ${:.2})",
            bet_title, bet_outcome, trade.size, trade.price, total_value
        );
    }

    // Only profile users if their trade value is >= our threshold
    if total_value >= config.large_trade_threshold {
        // If we haven't checked this user recently, fetch their history in background
        if !recent_users.contains(&trade.maker_address) {
            if should_print_trade {
                println!(
                    "  🤑 WHALE ALERT! New large trader detected! Fetching activity profile for {}...",
                    trade.maker_address
                );
            } else {
                println!(
                    "  🤑 WHALE ALERT (filtered trade) -> {} [{}] | Value: ${:.2} | Maker: {}",
                    bet_title, bet_outcome, total_value, trade.maker_address
                );
            }

            // Mark them as seen in the cache before spawning a new task to fetch their profile,
            // so we don't fire off multiple requests for the same user if they have multiple large trades in a short period.
            recent_users.put(trade.maker_address.clone(), ());

            // we might need the client and the adress outside of this async block, so we clone them here to move into the new task
            let client_clone = client.clone();
            let address_clone = trade.maker_address.clone();
            let user_history_db_clone = user_history_db.clone();
            // we also clone the relevant config values since we can't move the whole config struct into the async block
            let api_url = config.polymarket_data_api_url.clone();

            // tokio magic
            tokio::spawn(async move {
                match polymarket::fetch_user_activity(
                    &client_clone,
                    &api_url,
                    address_clone.as_str(),
                )
                .await
                {
                    Ok(activity) => {
                        if let Err(e) = user_history_db_clone
                            .insert_user_activity_snapshot(address_clone.as_str(), &activity)
                        {
                            eprintln!(
                                "  -> Failed to persist activity snapshot for {}: {:?}",
                                address_clone, e
                            );
                        }

                        // We get the activity object and convert it to a string,
                        let json_str = serde_json::to_string(&activity).unwrap_or_default();
                        // then we preview the first 200 characters in the console log so we can get a
                        // quick sense of the user's recent activity without overwhelming the terminal with a full JSON dump.
                        let preview: String = json_str.chars().take(200).collect();
                        println!(
                            "  -> Profile fetched for {}! Preview: {}...",
                            address_clone, preview
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "  -> Failed to fetch activity for {}: {:?}",
                            address_clone, e
                        );
                    }
                }
            });
        }
    }
}
