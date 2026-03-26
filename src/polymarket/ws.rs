use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc::Sender;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::Trade;

/// WebSocket message from Polymarket trades stream
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TradeEvent {
    #[serde(flatten)]
    pub trade: Trade,
}

pub struct WsConfig {
    pub ws_url: String,
    pub market_ids: Vec<String>,
}

/// Connect to the Polymarket market WebSocket and listen for trade events.
/// Sends trades into the channel and handles reconnection with exponential backoff.
pub async fn listen_market_trades(
    config: WsConfig,
    tx: Sender<Trade>,
) -> Result<()> {
    let mut backoff_secs = 1u32;
    let max_backoff_secs = 30u32;

    loop {
        // Add a timeout wrapper around connection attempts
        match tokio::time::timeout(Duration::from_secs(10), connect_and_subscribe(&config, tx.clone())).await {
            Ok(Ok(_)) => {
                // Connection closed normally or after a read error
                backoff_secs = 1; // Reset backoff on successful reconnect
            }
            Ok(Err(e)) => {
                eprintln!("[WS] Connection error: {:?}, retrying in {}s", e, backoff_secs);
                sleep(Duration::from_secs(backoff_secs as u64)).await;
                backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
            }
            Err(_) => {
                eprintln!("[WS] Connection timeout (10s), retrying in {}s", backoff_secs);
                sleep(Duration::from_secs(backoff_secs as u64)).await;
                backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
            }
        }
    }
}

async fn connect_and_subscribe(config: &WsConfig, tx: Sender<Trade>) -> Result<()> {
    println!("[WS] Connecting to {}", config.ws_url);
    let (ws_stream, _) = connect_async(&config.ws_url)
        .await
        .context("Failed to connect to WebSocket")?;

    println!("[WS] Connected. Subscribing to market trades...");

    let (mut write, mut read) = ws_stream.split();

    // Send subscription message
    let subscribe_msg = json!({
        "assets_ids": config.market_ids,
        "type": "market",
        "custom_feature_enabled": true
    });

    write
        .send(Message::Text(subscribe_msg.to_string()))
        .await
        .context("Failed to send subscription message")?;

    println!("[WS] Subscription sent. Listening for trades...");

    // Read messages from the stream
    let mut msg_count = 0;
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                msg_count += 1;
                // Log raw message every 50 messages or on first few to debug format
                if msg_count <= 3 || msg_count % 50 == 0 {
                    eprintln!("[WS] Raw message #{}: {}", msg_count, &text[..std::cmp::min(500, text.len())]);
                }
                
                // Try to parse as a single trade
                match serde_json::from_str::<Trade>(&text) {
                    Ok(trade) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        let lag = now.saturating_sub(trade.timestamp);
                        println!("[WS] Trade received: tx={} lag={}s size={} price={}", 
                            &trade.transaction_hash[..10.min(trade.transaction_hash.len())], 
                            lag, trade.size, trade.price);
                        
                        if let Err(e) = tx.send(trade).await {
                            eprintln!("[WS] Failed to send trade to channel: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        // Might be a different message type, log occasionally
                        if msg_count <= 5 || msg_count % 100 == 0 {
                            eprintln!("[WS] Failed to parse as Trade (msg #{}): {}", msg_count, e);
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => {
                println!("[WS] WebSocket closed by server after {} messages", msg_count);
                break;
            }
            Ok(_) => {
                // Ignore ping/pong and other messages
            }
            Err(e) => {
                eprintln!("[WS] WebSocket read error after {} messages: {}", msg_count, e);
                return Err(anyhow::anyhow!("WebSocket read error: {}", e));
            }
        }
    }

    Ok(())
}
