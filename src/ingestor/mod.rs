use anyhow::{Context, Result};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::data_structures::Trade;
use crate::database_handler::DatabaseHandler;

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
    pub fn ingest_raw_json_str(
        &self,
        raw_payload: &str,
        db_handler: &DatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        let raw_value: Value =
            serde_json::from_str(raw_payload).context("Failed to parse raw trade payload text")?;
        self.ingest_raw_value(raw_value, db_handler)
    }

    /// Parse a raw JSON value returned by the API into normalized trades.
    pub fn ingest_raw_value(
        &self,
        raw_payload: Value,
        db_handler: &DatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        let trades: Vec<Trade> = serde_json::from_value(raw_payload)
            .context("Failed to deserialize raw payload into trades")?;
        self.persist_batch(db_handler, &trades)?;
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(trades, ingested_at))
    }

    /// Normalize already-deserialized trades (for call sites using fetch_global_trades).
    #[cfg(any())]
    pub fn ingest_trades(
        &self,
        trades: Vec<Trade>,
        db_handler: &DatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        self.persist_batch(db_handler, &trades)?;
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(trades, ingested_at))
    }

    fn persist_batch(&self, db_handler: &DatabaseHandler, trades: &[Trade]) -> Result<()> {
        for trade in trades {
            db_handler.insert_trade(trade)?;
        }
        Ok(())
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
