use crate::data_structures::WalletAddress;
use crate::polymarket;
use crate::polymarket::{ClosedPosition, PastTrade, Trade};
use anyhow::Result;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct InvestigationRequest {
    trade: Trade,
    p_value: f64,
}

impl InvestigationRequest {
    pub fn new(trade: Trade, p_value: f64) -> Self {
        Self { trade, p_value }
    }

    pub fn address(&self) -> &WalletAddress {
        &self.trade.maker_address
    }

    pub fn trade(&self) -> &Trade {
        &self.trade
    }

    pub fn p_value(&self) -> f64 {
        self.p_value
    }
}

#[derive(Debug, Clone)]
pub struct UserActivityReport {
    pub address: WalletAddress,
    pub past_trades: Vec<PastTrade>,
    pub closed_positions: Vec<ClosedPosition>,
    pub win_rate: f64,
    pub p_value: f64,
}

/// Orchestrator function to fetch user history and return a typed report.
pub async fn fetch_user_history(
    client: reqwest::Client,
    api_url: String,
    address: WalletAddress,
) -> Result<Vec<PastTrade>> {
    let raw_activity = polymarket::fetch_user_activity(&client, &api_url, address.as_str()).await?;

    // We convert the Vec<Value> into Vec<PastTrade> using serde_json::from_value
    let past_trades: Vec<PastTrade> =
        serde_json::from_value(serde_json::Value::Array(raw_activity))?;
    Ok(past_trades)
}

/// Orchestrator function to fetch user resolved positions.
pub async fn fetch_user_closed_positions(
    client: reqwest::Client,
    api_url: String,
    address: WalletAddress,
) -> Result<Vec<ClosedPosition>> {
    polymarket::fetch_user_closed_positions(&client, &api_url, address.as_str()).await
}

/// Placeholder for user custom win-rate logic.
pub fn calculate_win_rate(_positions: &[ClosedPosition], _past_trades: &[PastTrade]) -> f64 {
    // User will implement this logic based on curPrice and endDate
    // the best way to do this is by having past_trades be a hashmap from some id to the trade
    // details, and then for each closed position, we can look up the corresponding trade
    // details to determine if the last trade on that market for this user was a buy or a sell.
    0.0
}

/// Spawns a background task to conduct a multi-source parallel investigation.
pub fn spawn_investigation(
    req: InvestigationRequest,
    client: reqwest::Client,
    api_url: String,
) -> JoinHandle<Result<UserActivityReport>> {
    tokio::spawn(async move {
        let address = req.address().clone();
        let p_value = req.p_value();

        // Parallel Data Fetching
        let (past_trades, closed_positions) = tokio::try_join!(
            fetch_user_history(client.clone(), api_url.clone(), address.clone()),
            fetch_user_closed_positions(client.clone(), api_url.clone(), address.clone())
        )?;

        let win_rate = calculate_win_rate(&closed_positions, &past_trades);

        Ok(UserActivityReport {
            address,
            past_trades,
            closed_positions,
            win_rate,
            p_value,
        })
    })
}
