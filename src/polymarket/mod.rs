use anyhow::{Context, Result};
use futures_util::{stream::StreamExt, SinkExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Serialize, Debug)]
pub struct WsSubscribeMessage {
    pub assets_ids: Vec<String>,
    #[serde(rename = "type")]
    pub msg_type: String, // usually "market"
}

#[derive(Deserialize, Debug)]
pub struct WsEvent {
    pub event: String,                   // "trade", "price_change", etc.
    pub asset_id: Option<String>,
    pub maker_address: Option<String>,
    pub price: Option<String>,
    pub size: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Deserialize, Debug)]
pub struct ActivityResponse {
    // We will inspect this payload during runtime to see what it contains
    #[serde(flatten)]
    pub payload: Value,
}

/// Connects to the Polymarket WebSocket and subscribes to the given markets.
pub async fn subscribe_to_markets(
    ws_url: &str,
    markets: &[String],
) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>> {
    let (mut ws_stream, _) = connect_async(ws_url)
        .await
        .context("Failed to connect to Polymarket WebSocket")?;

    let sub_msg = WsSubscribeMessage {
        assets_ids: markets.to_vec(),
        msg_type: "market".to_string(),
    };

    let msg_str = serde_json::to_string(&sub_msg)?;
    ws_stream.send(Message::Text(msg_str.into())).await?;

    Ok(ws_stream)
}

/// Fetches user activity from the Data API
pub async fn fetch_user_activity(
    client: &reqwest::Client,
    data_api_url: &str,
    user_address: &str,
) -> Result<ActivityResponse> {
    let url = format!("{}/activity?user={}", data_api_url, user_address);
    let res = client.get(&url).send().await?.error_for_status()?;
    let activity = res.json::<ActivityResponse>().await?;
    Ok(activity)
}
