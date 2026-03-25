#![allow(warnings)]

mod config;
mod polymarket;

use anyhow::Result;
use config::Config;
use lru::LruCache;
use polymarket::Trade;
use std::num::NonZeroUsize;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load configuration
    let config = Config::load()?;
    println!("Loaded config! Polling Global Trades API every {} seconds", config.poll_interval_secs);

    // 2. Set up our LRU (Least Recently Used) Caches
    // This cache limits memory usage by only remembering the 1,000 most recently queried addresses
    let mut recent_users: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(1000).unwrap());
    
    // We also need a cache to remember which trades we've already processed, 
    // so our 5-second polling loop doesn't double-count the same trade.
    let mut processed_trades: LruCache<String, ()> = 
        LruCache::new(NonZeroUsize::new(5000).unwrap());

    // 3. Create a shared HTTP Client for the application
    let http_client = reqwest::Client::new();
    
    // 4. Create our polling interval
    let mut poll_interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    // 5. Background Polling Loop
    println!("Starting global trade Watchdog... Polling every {}s", config.poll_interval_secs);
    loop {
        poll_interval.tick().await;
        println!("--> Polling Data API for new trades...");
        
        // Fetch the last 100 global trades across all of Polymarket
        match polymarket::fetch_global_trades(&http_client, &config.polymarket_data_api_url, 100).await {
            Ok(trades) => {
                let mut new_trades_count = 0;
                // Since trades are usually returned newest-first, we process them in reverse
                // so the console output reads in actual chronological order.
                for trade in trades.into_iter().rev() {
                    // Have we already seen this trade in the last interval?
                    if !processed_trades.contains(&trade.transaction_hash) {
                        processed_trades.put(trade.transaction_hash.clone(), ());
                        new_trades_count += 1;
                        
                        // Fire off to our central processing logic
                        handle_trade(&mut recent_users, http_client.clone(), config.clone(), trade);
                    }
                }
                
                if new_trades_count == 0 {
                    println!("    (No new trades in the last {} seconds)", config.poll_interval_secs);
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
    client: reqwest::Client,
    config: Config,
    trade: Trade,
) {
    let total_value = trade.size * trade.price;
    let bet_title = trade.title.as_deref().unwrap_or("Unknown Market");
    let bet_outcome = trade.outcome.as_deref().unwrap_or("N/A");

    println!(
        "🚨 TRADE -> {} [{}] | Share Size: {:.2} @ Price: ${:.2} (Value: ${:.2}) | Vol: {} | Maker: {}",
        bet_title,
        bet_outcome,
        trade.size,
        trade.price,
        total_value,
        trade.asset,
        trade.maker_address
    );

    // Only profile users if their trade value is >= our threshold
    if total_value >= config.large_trade_threshold {
        // If we haven't checked this user recently, fetch their history in background
        if !recent_users.contains(&trade.maker_address) {
            println!("  🤑 WHALE ALERT! New large trader detected! Fetching activity profile for {}...", trade.maker_address);
            
            // Mark them as seen in the cache immediately so we don't spawn multiple tasks
            recent_users.put(trade.maker_address.clone(), ());

            let client_clone = client.clone();
            let address_clone = trade.maker_address.clone();
            let api_url = config.polymarket_data_api_url.clone();
            
            tokio::spawn(async move {
                match polymarket::fetch_user_activity(&client_clone, &api_url, &address_clone).await {
                    Ok(activity) => {
                        // Log a snippet of their history
                        let json_str = serde_json::to_string(&activity).unwrap_or_default();
                        let preview: String = json_str.chars().take(200).collect();
                        println!("  -> Profile fetched for {}! Preview: {}...", address_clone, preview);
                    }
                    Err(e) => {
                        eprintln!("  -> Failed to fetch activity for {}: {:?}", address_clone, e);
                    }
                }
            });
        }
    }
}
