#![allow(warnings)]

mod config;
mod db;
mod polymarket;
mod trade_filter;

use anyhow::Result;
use config::Config;
use lru::LruCache;
use polymarket::Trade;
use std::num::NonZeroUsize;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};
use trade_filter::TradeFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load configuration
    let config = Config::load()?;
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
    let mut recent_users: LruCache<String, ()> = LruCache::new(NonZeroUsize::new(1000).unwrap());

    // We also need a cache to remember which trades we've already processed,
    // so our 5-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(5000).unwrap());

    // 3. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    let trade_filter = TradeFilter::new();

    // 4. Create our polling interval
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 5. Background Polling Loop
    println!(
        "Starting global trade Watchdog... Polling every {}s",
        config.poll_interval_secs
    );
    let mut stale_streak: u32 = 0;
    loop {
        poll_interval.tick().await;
        println!("--> Polling Data API for new trades...");

        // Fetch the latest global trades across all of Polymarket
        match polymarket::fetch_global_trades(
            &http_client,
            &config.polymarket_data_api_url,
            config.global_trades_limit,
        )
        .await
        {
            Ok(trades) => {
                let mut new_trades_count = 0;

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

/// Core application logic for processing a new trade
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

    if should_print_trade {
        println!(
            "🚨 TRADE -> {} [{}] | Share Size: {:.2} @ Price: ${:.2} (Value: ${:.2}) | Maker: {}",
            bet_title, bet_outcome, trade.size, trade.price, total_value, trade.maker_address
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

            // Mark them as seen in the cache immediately so we don't spawn multiple tasks
            recent_users.put(trade.maker_address.clone(), ());

            let client_clone = client.clone();
            let address_clone = trade.maker_address.clone();
            let api_url = config.polymarket_data_api_url.clone();
            let db_path = config.sqlite_db_path.clone();

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

                        // Log a snippet of their history
                        let json_str = serde_json::to_string(&activity).unwrap_or_default();
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
