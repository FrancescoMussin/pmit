use serde::{Deserialize, Serialize};

/// Trade payload shared across ingestion, persistence, and downstream stages.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Trade {
    // The API key is `proxyWallet`; keep serde alias for compatibility.
    #[serde(alias = "proxyWallet")]
    pub maker_address: String,
    pub side: String,
    pub asset: String,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub size: f64,
    pub price: f64,
    pub timestamp: u64,
    #[serde(alias = "transactionHash")]
    pub transaction_hash: String,
}
