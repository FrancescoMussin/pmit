use anyhow::{Context, Result};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::data_structures::Trade;

/// Normalized batch produced by the ingestor.
///
/// One API payload becomes one batch with a single ingestion timestamp.
#[derive(Debug, Clone)]
pub struct IngestedTradeBatch {
    trades: Vec<Trade>,
    ingested_at_unix_s: u64,
}

impl IngestedTradeBatch {
    pub fn new(trades: Vec<Trade>, ingested_at_unix_s: u64) -> Self {
        Self {
            trades,
            ingested_at_unix_s,
        }
    }

    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }

    pub fn into_trades(self) -> Vec<Trade> {
        self.trades
    }

    pub fn ingested_at_unix_s(&self) -> u64 {
        self.ingested_at_unix_s
    }
}

/// Converts raw API payloads into normalized trades for downstream stages.
#[derive(Debug, Default, Clone)]
pub struct TradeIngestor;

impl TradeIngestor {
    pub fn new() -> Self {
        Self
    }

    /// Parse raw JSON payload text returned by the API into normalized trades.
    pub fn ingest_raw_json_str(&self, raw_payload: &str) -> Result<IngestedTradeBatch> {
        let raw_value: Value =
            serde_json::from_str(raw_payload).context("Failed to parse raw trade payload text")?;
        self.ingest_raw_value(raw_value)
    }

    /// Parse a raw JSON value returned by the API into normalized trades.
    pub fn ingest_raw_value(&self, raw_payload: Value) -> Result<IngestedTradeBatch> {
        let trades: Vec<Trade> = serde_json::from_value(raw_payload)
            .context("Failed to deserialize raw payload into trades")?;
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(trades, ingested_at))
    }

    /// Normalize already-deserialized trades (for call sites using fetch_global_trades).
    pub fn ingest_trades(&self, trades: Vec<Trade>) -> Result<IngestedTradeBatch> {
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(trades, ingested_at))
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
