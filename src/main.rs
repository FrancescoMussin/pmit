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
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use trade_filter::TradeFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let db = db::Database::new(&config.sqlite_db_path)?;
    
    println!(
        "Loaded config! Polling Global Trades API every {} seconds (limit={})",
        config.poll_interval_secs, config.global_trades_limit
    );
    println!("SQLite storage initialized at {}", config.sqlite_db_path);
    println!(
        "Staleness detector: warn_lag={}s, consecutive_polls={}",
        config.stale_feed_warn_secs, config.stale_feed_consecutive_polls
    );

    let mut recent_users: LruCache<String, ()> = LruCache::new(NonZeroUsize::new(1000).unwrap());
    let mut processed_trades: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(5000).unwrap());

    let http_client = reqwest::Client::new();
    let trade_filter = TradeFilter::new();

    // Create a channel for trades from both WS and polling sources
    let (tx, mut rx) = mpsc::channel::<Trade>(1000);

    // Spawn WebSocket listener task (disabled for now - TLS issues)
    // if !config.polymarket_market_ids.is_empty() {
    //     let ws_config = polymarket::ws::WsConfig {
    //         ws_url: config.polymarket_ws_url.clone(),
    //         market_ids: config.polymarket_market_ids.clone(),
    //     };
    //     let tx_ws = tx.clone();
    //     tokio::spawn(async move {
    //         if let Err(e) = polymarket::ws::listen_market_trades(ws_config, tx_ws).await {
    //             eprintln!("[WS] Fatal error: {:?}", e);
    //         }
    //     });
    // }

    // Spawn polling task
    let tx_poll = tx.clone();
    let config_poll = config.clone();
    let http_client_poll = http_client.clone();
    tokio::spawn(async move {
        let mut poll_interval = tokio::time::interval(Duration::from_secs(config_poll.poll_interval_secs));
        let mut stale_streak: u32 = 0;
        println!("[POLL] Polling task started");

        loop {
            poll_interval.tick().await;
            println!("[POLL] Polling Data API for new trades...");

            match polymarket::fetch_global_trades(
                &http_client_poll,
                &config_poll.polymarket_data_api_url,
                config_poll.global_trades_limit,
            )
            .await
            {
                Ok(trades) => {
                    println!("[POLL] Fetched {} trades from REST API", trades.len());
                    if let Some(newest_trade_ts) = trades.iter().map(|t| t.timestamp).max() {
                        let now_ts = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let lag_secs = now_ts.saturating_sub(newest_trade_ts);

                        if lag_secs >= config_poll.stale_feed_warn_secs {
                            stale_streak += 1;
                            eprintln!(
                                "[STALE FEED] newest trade is {}s old (threshold={}s, streak={})",
                                lag_secs, config_poll.stale_feed_warn_secs, stale_streak
                            );
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

                    for trade in trades {
                        if tx_poll.send(trade).await.is_err() {
                            eprintln!("[POLL] Failed to send poll trade to channel");
                            break;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[POLL] Error fetching global trades: {:?}", e);
                }
            }
        }
    });

    // Main trade processing loop - consume from both WS and polling
    println!("Starting trade processor...");
    while let Some(trade) = rx.recv().await {
        if !processed_trades.contains(&trade.transaction_hash) {
            processed_trades.put(trade.transaction_hash.clone(), ());

            if let Err(e) = db.insert_trade(&trade) {
                eprintln!(
                    "Failed to persist trade {}: {:?}",
                    trade.transaction_hash, e
                );
            }

            handle_trade(
                &db,
                &mut recent_users,
                &trade_filter,
                http_client.clone(),
                config.clone(),
                trade,
            );
        } else {
            // Trade already processed, skip
        }
    }

    Ok(())
}

fn handle_trade(
    db: &db::Database,
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
            "🚨 TRADE: {} [{}] | Share Size: {:.2} @ Price: ${:.2} (Value: ${:.2})",
            bet_title, bet_outcome, trade.size, trade.price, total_value
        );
    }

    if total_value >= config.large_trade_threshold {
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

            recent_users.put(trade.maker_address.clone(), ());

            let client_clone = client.clone();
            let address_clone = trade.maker_address.clone();
            let db_clone = db.clone();
            let api_url = config.polymarket_data_api_url.clone();

            tokio::spawn(async move {
                match polymarket::fetch_user_activity(&client_clone, &api_url, &address_clone).await
                {
                    Ok(activity) => {
                        if let Err(e) =
                            db_clone.insert_user_activity_snapshot(&address_clone, &activity)
                        {
                            eprintln!(
                                "  -> Failed to persist activity snapshot for {}: {:?}",
                                address_clone, e
                            );
                        }

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
