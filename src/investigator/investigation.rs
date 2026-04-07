use crate::data_structures::WalletAddress;
use crate::polymarket;
use crate::polymarket::{ClosedPosition, PastTrade, Trade};
use crate::data_structures::trade::ClobTokenId;
use std::collections::HashMap;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PastUserTrades {
    pub trades_by_token: HashMap<ClobTokenId, Vec<PastTrade>>,
}

impl PastUserTrades {
    pub fn len(&self) -> usize {
        self.trades_by_token.values().map(|v: &Vec<PastTrade>| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicProfileData {
    pub created_at: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct UserActivityReport {
    pub address: WalletAddress,
    pub past_trades: PastUserTrades,
    pub closed_positions: Vec<ClosedPosition>,
    pub public_profile: PublicProfileData,
    pub win_rate: f64,
    pub p_value: f64,
}

/// Orchestrator function to fetch user history and return a typed report.
pub async fn fetch_user_history(
    client: reqwest::Client,
    api_url: String,
    address: WalletAddress,
) -> Result<PastUserTrades> {
    let raw_activity = polymarket::fetch_user_activity(&client, &api_url, address.as_str()).await?;

    // We convert the Vec<Value> into Vec<PastTrade> using serde_json::from_value
    let flat_trades: Vec<PastTrade> =
        serde_json::from_value(serde_json::Value::Array(raw_activity))?;
        
    let mut trades_by_token = HashMap::new();
    for trade in flat_trades {
        trades_by_token.entry(trade.asset.clone()).or_insert_with(Vec::new).push(trade);
    }
    
    Ok(PastUserTrades { trades_by_token })
}

pub async fn fetch_public_profile(
    client: reqwest::Client,
    gamma_url: String,
    address: WalletAddress,
) -> Result<PublicProfileData> {
    let url = format!("{}/public-profile?address={}", gamma_url, address.as_str());
    let res = client.get(&url).send().await?;
    
    // An address might not have a public profile setup.
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(PublicProfileData {
            created_at: None,
            name: address.to_string(),
        });
    }

    let profile = res.error_for_status()?.json::<PublicProfileData>().await?;
    
    Ok(profile)
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
pub fn calculate_win_rate(positions: &[ClosedPosition], past_trades: &PastUserTrades) -> f64 {
    let mut wins = 0.0;
    let mut total_bets = 0.0;
    let now = chrono::Utc::now();

    // Here's how the algorithm works:
    // 1. For each closed position, we first check that the end date is past the current date.
    // 2. We then look at the curPrice field. If it's 1, it means that the user predicted the
    // event correctly, otherwise if it's 0 it means that the user predicted the event incorrectly,
    // if it's anything other than that it means that the event has not resolved yet for some reason and
    // that clobtokenId should be ignored.
    // 3. Finally, to see if the user held on to the position until event resolution, we look at the first
    // trade saved in the PastUserTrades at that clobtokenId. If the side is a side::buy, then it means
    // that the user held on until the end and we increase the wins counter and total bets counter. 
    // Otherwise it means that the user sold off before the event 
    // resolution and we don't count this.
    // 
    // we return win_rate/total_bets as f64.
    
    for pos in positions {
        if let Ok(end_date) = pos.end_date.parse::<chrono::DateTime<chrono::Utc>>() {
            if end_date > now {
                continue;
            }
        } else {
            continue;
        }

        let close_to_one = (pos.cur_price - 1.0).abs() < f64::EPSILON;
        let close_to_zero = (pos.cur_price).abs() < f64::EPSILON;
        if close_to_one || close_to_zero {
            if let Some(trades) = past_trades.trades_by_token.get(&pos.asset) {
                if let Some(first_trade) = trades.first() {
                    if first_trade.side == crate::polymarket::Side::Buy {
                        total_bets += 1.0;
                        if close_to_one {
                            wins += 1.0;
                        }
                    }
                }
            }
        }
    }

    if total_bets > 0.0 {
        wins / total_bets
    } else {
        0.0
    }
}

/// Spawns a background task to conduct a multi-source parallel investigation.
pub fn spawn_investigation(
    req: InvestigationRequest,
    client: reqwest::Client,
    api_url: String,
    gamma_url: String,
) -> JoinHandle<Result<UserActivityReport>> {
    tokio::spawn(async move {
        let address = req.address().clone();
        let p_value = req.p_value();

        // Parallel Data Fetching
        let (past_trades, closed_positions, public_profile) = tokio::try_join!(
            fetch_user_history(client.clone(), api_url.clone(), address.clone()),
            fetch_user_closed_positions(client.clone(), api_url.clone(), address.clone()),
            fetch_public_profile(client.clone(), gamma_url.clone(), address.clone())
        )?;

        let win_rate = calculate_win_rate(&closed_positions, &past_trades);

        Ok(UserActivityReport {
            address,
            past_trades,
            closed_positions,
            public_profile,
            win_rate,
            p_value,
        })
    })
}
