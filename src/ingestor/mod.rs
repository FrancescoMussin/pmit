use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::data_structures::{Side, Trade};
use crate::database_handler::TradeDatabaseHandler;

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
        db_handler: &TradeDatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        let raw_value: Value =
            serde_json::from_str(raw_payload).context("Failed to parse raw trade payload text")?;
        self.ingest_raw_value(raw_value, db_handler)
    }

    /// Parse a raw JSON value returned by the API into normalized trades.
    pub fn ingest_raw_value(
        &self,
        raw_payload: Value,
        db_handler: &TradeDatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        let buy_trades = deserialize_buy_trades(raw_payload)?;
        self.persist_batch(db_handler, &buy_trades)?;
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(buy_trades, ingested_at))
    }

    /// Normalize already-deserialized trades (for call sites using fetch_global_trades).
    #[cfg(any())]
    pub fn ingest_trades(
        &self,
        trades: Vec<Trade>,
        db_handler: &TradeDatabaseHandler,
    ) -> Result<IngestedTradeBatch> {
        self.persist_batch(db_handler, &trades)?;
        let ingested_at = now_ts();
        Ok(IngestedTradeBatch::new(trades, ingested_at))
    }

    fn persist_batch(&self, db_handler: &TradeDatabaseHandler, trades: &[Trade]) -> Result<()> {
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

fn deserialize_buy_trades(raw_payload: Value) -> Result<Vec<Trade>> {
    let Value::Array(items) = raw_payload else {
        return Err(anyhow!("Expected trade payload to be a JSON array"));
    };

    let mut buy_trades = Vec::new();

    for item in items {
        let side = item
            .get("side")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Trade item missing side field"))?;

        if !side.eq_ignore_ascii_case("buy") {
            continue;
        }

        let trade: Trade = serde_json::from_value(item)
            .context("Failed to deserialize buy trade from payload item")?;

        if trade.side == Side::Buy {
            buy_trades.push(trade);
        }
    }

    Ok(buy_trades)
}
