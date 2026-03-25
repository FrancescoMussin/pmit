mod config;
mod polymarket;

use anyhow::Result;
use config::Config;
use futures_util::StreamExt;
use lru::LruCache;
use std::num::NonZeroUsize;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::tungstenite::protocol::Message;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Load configuration
    let config = Config::load()?;
    println!("Loaded config: {} markets to monitor", config.markets.len());

    // 2. Initialize LRU Cache (to prevent spamming API for same users)
    // Cache up to 1000 addresses
    let mut recent_users: LruCache<String, ()> =
        LruCache::new(NonZeroUsize::new(1000).unwrap());

    // 3. Setup HTTP Client for Data API
    let http_client = reqwest::Client::new();

    // 4. Connect to WebSocket
    println!("Connecting to Polymarket WebSocket...");
    let mut ws_stream = polymarket::subscribe_to_markets(
        &config.polymarket_ws_url,
        &config.markets,
    ).await?;
    println!("Connected and subscribed to markets: {:?}", config.markets);

    // 5. Processing Loop
    while let Some(msg_result) = ws_stream.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                println!("RAW WS TEXT: {}", text);
                // Let's parse the JSON to inspect the structure
                if let Ok(json_array) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    for event in json_array {
                        // Check if it's a trade event and extract maker_address
                        if let Some(event_type) = event.get("event").and_then(|v| v.as_str()) {
                            if event_type == "trade" {
                                if let Some(maker_address) = event.get("maker_address").and_then(|v| v.as_str()) {
                                    handle_trade(&mut recent_users, http_client.clone(), config.clone(), maker_address.to_string(), event.clone());
                                }
                            } else {
                                // Just log other events temporarily to understand the stream
                                // println!("Ignored event '{}': {:?}", event_type, event);
                            }
                        }
                    }
                } else {
                    println!("Received non-array text message: {}", text);
                }
            }
            Ok(Message::Ping(p)) => {
                // Tungstenite usually handles pongs automatically, but we can log
                println!("Ping received");
            }
            Ok(Message::Close(c)) => {
                println!("WebSocket closed remotely: {:?}", c);
                break;
            }
            Err(e) => {
                eprintln!("WebSocket Error: {:?}", e);
            }
            _ => {}
        }
    }

    Ok(())
}

fn handle_trade(
    recent_users: &mut LruCache<String, ()>,
    client: reqwest::Client,
    config: Config,
    address: String,
    event: serde_json::Value,
) {
    let size_str = event.get("size").and_then(|v| v.as_str()).unwrap_or("0");
    let price_str = event.get("price").and_then(|v| v.as_str()).unwrap_or("0");
    
    let size = size_str.parse::<f64>().unwrap_or(0.0);
    let price = price_str.parse::<f64>().unwrap_or(0.0);
    let total_value = size * price;

    println!("🚨 TRADE EXECUTED -> Shares: {} @ Price: {} (Value: ${:.2}) | Maker: {}", size_str, price_str, total_value, address);

    // Only profile users if their trade value is >= our threshold
    if total_value >= config.large_trade_threshold {
        // If we haven't checked this user recently, fetch their history in background
        if !recent_users.contains(&address) {
            println!("  🤑 WHALE ALERT! New large trader detected! Fetching activity profile for {}...", address);
            // Mark them as seen in the cache immediately so we don't spawn multiple tasks
            recent_users.put(address.clone(), ());

            let client_clone = client.clone();
            tokio::spawn(async move {
                match polymarket::fetch_user_activity(&client_clone, &config.polymarket_data_api_url, &address).await {
                    Ok(activity) => {
                        // Log a snippet of their history
                        let json_str = serde_json::to_string(&activity.payload).unwrap_or_default();
                        let preview: String = json_str.chars().take(200).collect();
                        println!("  -> Profile fetched for {}! Preview: {}...", address, preview);
                    }
                    Err(e) => {
                        eprintln!("  -> Failed to fetch activity for {}: {:?}", address, e);
                    }
                }
            });
        }
    } else {
        // Small trade, ignored for profiling
        // println!("  -> Small trade (${:.2}), ignored user profiling.", total_value);
    }
}
