#![allow(warnings)]

mod config;
mod data_structures;
mod db;
mod ingestor;
mod polymarket;
mod trade_filter;

use anyhow::Result;
use config::Config;
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
    // we also initialize our SQLite database and schema before starting the polling loop
    db::init(&config.sqlite_db_path)?;
    println!(
        "Loaded config! Polling Global Trades API every {} seconds (limit={})",
        config.poll_interval_secs, config.global_trades_limit
    );
    println!("SQLite storage initialized at {}", config.sqlite_db_path);
    println!(
        "Staleness detector: warn_lag={}s, consecutive_polls={}",
        config.stale_feed_warn_secs, config.stale_feed_consecutive_polls
    );
    // 2. Set up our LRU (Least Recently Used) Caches
    // This cache limits memory usage by only remembering the 1,000 most recently queried addresses
    // the value here is () because only membership in the cache matters, not the value itself
    let mut recent_users: LruCache<String, ()> = LruCache::new(NonZeroUsize::new(1000).unwrap());

    // We also need a cache to remember which trades we've already processed,
    // so our 10-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(5000).unwrap());

    // 3. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    let trade_ingestor = TradeIngestor::new();
    // we can initialize the trade filter
    let trade_filter = TradeFilter::new();

    // 4. Create our polling interval
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 5. Background Polling Loop
    println!(
        "Starting global trade Watchdog... Polling every {}s",
        config.poll_interval_secs
    );
    // this is just for keeping track of how many consecutive times we've seen a stale feed, so we can log that pattern if it emerges
    let mut stale_streak: u32 = 0;
    // We start the polling loop, which will run indefinitely until the program is stopped
    loop {
        // this blocks a new iteration of the loop until the configured interval has passed since the last tick
        poll_interval.tick().await;
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
                let ingested_batch = match trade_ingestor.ingest_raw_value(raw_payload) {
                    Ok(batch) => batch,
                    Err(e) => {
                        eprintln!("Error ingesting raw trades payload: {:?}", e);
                        continue;
                    }
                };
                let trades = ingested_batch.into_trades();

                let mut new_trades_count = 0;
                // we look at the newest trade's timestamp to see how fresh the feed is. If it's older than our configured threshold, we log a warning.
                // If this happens for multiple consecutive polls, we log that pattern as well since it often indicates an issue with CDN caching.
                if let Some(newest_trade_ts) = trades.iter().map(|t| t.timestamp).max() {
                    let now_ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let lag_secs = now_ts.saturating_sub(newest_trade_ts);

                    if lag_secs >= config.stale_feed_warn_secs {
                        stale_streak += 1;
                        eprintln!(
                            "[STALE FEED] newest trade is {}s old (threshold={}s, streak={})",
                            lag_secs, config.stale_feed_warn_secs, stale_streak
                        );

                        if stale_streak >= config.stale_feed_consecutive_polls {
                            eprintln!(
                                "[STALE FEED] sustained staleness detected. This often indicates CDN/API cache windows."
                            );
                        }
                    } else {
                        if stale_streak > 0 {
                            println!(
                                "Feed freshness recovered after {} stale poll(s). Current lag={}s",
                                stale_streak, lag_secs
                            );
                        }
                        stale_streak = 0;
                    }
                }

                // Since trades are usually returned newest-first, we process them in reverse
                // so the console output reads in actual chronological order.
                for trade in trades.into_iter().rev() {
                    // Have we already seen this trade in the last interval?
                    if !processed_trades.contains(&trade.transaction_hash) {
                        processed_trades.put(trade.transaction_hash.clone(), ());
                        new_trades_count += 1;

                        if let Err(e) = db::insert_trade(&config.sqlite_db_path, &trade) {
                            eprintln!(
                                "Failed to persist trade {}: {:?}",
                                trade.transaction_hash, e
                            );
                        }

                        // Fire off to our central processing logic
                        // this prints out the trade to terminal if it passes the filter,
                        // and also checks if we should profile the user behind the trade based on our configured threshold,
                        // in which case this spawns a new asynchronous task to fetch their history and persist it to the database.
                        handle_trade(
                            &mut recent_users,
                            &trade_filter,
                            http_client.clone(),
                            config.clone(),
                            trade,
                        );
                    }
                }

                if new_trades_count == 0 {
                    println!(
                        "    (No new trades in the last {} seconds)",
                        config.poll_interval_secs
                    );
                }
            }
            Err(e) => {
                eprintln!("Error fetching global trades: {:?}", e);
            }
        }
    }
}

/// This function processes each trade.
/// First it prints out the trade details if it passes the trade_filter, then if the trade value is above our configured threshold,
/// it checks if we've recently profiled this user. If not, it spawns a new asynchronous task to fetch their activity and persist
/// it to the database, while also printing a preview of their profile to the console.
fn handle_trade(
    recent_users: &mut LruCache<String, ()>,
    trade_filter: &TradeFilter,
    client: reqwest::Client,
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
            // we also clone the relevant config values since we can't move the whole config struct into the async block
            let api_url = config.polymarket_data_api_url.clone();
            let db_path = config.sqlite_db_path.clone();

            // tokio magic
            tokio::spawn(async move {
                match polymarket::fetch_user_activity(&client_clone, &api_url, &address_clone).await
                {
                    Ok(activity) => {
                        if let Err(e) =
                            db::insert_user_activity_snapshot(&db_path, &address_clone, &activity)
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
