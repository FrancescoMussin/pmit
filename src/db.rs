use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::polymarket::Trade;

/// Holds a persistent database connection and provides methods for interacting with the SQLite database.
/// Uses Arc<Mutex<>> to allow cloning for use across async task boundaries.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Opens a connection to the SQLite database at the given path and initializes the schema.
    /// This creates a single persistent connection that is reused for all subsequent operations.
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = open(db_path)?;
        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Initializes the SQLite database schema. This creates the necessary tables and indexes if they don't already exist.
    fn init_schema(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "
        CREATE TABLE IF NOT EXISTS trades (
            transaction_hash TEXT PRIMARY KEY,
            maker_address TEXT NOT NULL,
            side TEXT NOT NULL,
            asset TEXT NOT NULL,
            title TEXT,
            outcome TEXT,
            size REAL NOT NULL,
            price REAL NOT NULL,
            timestamp INTEGER NOT NULL,
            raw_json TEXT NOT NULL,
            ingested_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_trades_timestamp ON trades(timestamp);
        CREATE INDEX IF NOT EXISTS idx_trades_maker_address ON trades(maker_address);

        CREATE TABLE IF NOT EXISTS user_activity_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_address TEXT NOT NULL,
            fetched_at INTEGER NOT NULL,
            raw_json TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_activity_user_time
            ON user_activity_snapshots(user_address, fetched_at);
        ",
        )
        .context("Failed to initialize SQLite schema")?;

        Ok(())
    }

    pub fn insert_trade(&self, trade: &Trade) -> Result<()> {
        let raw_json = serde_json::to_string(trade).context("Failed to serialize trade JSON")?;
        let ingested_at = now_ts();

        let mut conn = self.conn.lock().unwrap();
        conn.execute(
            "
            INSERT OR IGNORE INTO trades (
                transaction_hash, maker_address, side, asset, title, outcome,
                size, price, timestamp, raw_json, ingested_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ",
            params![
                trade.transaction_hash,
                trade.maker_address,
                trade.side,
                trade.asset,
                trade.title,
                trade.outcome,
                trade.size,
                trade.price,
                trade.timestamp,
                raw_json,
                ingested_at,
            ],
        )
        .context("Failed to insert trade")?;

        Ok(())
    }

    pub fn insert_user_activity_snapshot(
        &self,
        user_address: &str,
        activity: &[Value],
    ) -> Result<()> {
        let raw_json = serde_json::to_string(activity)
            .context("Failed to serialize user activity snapshot JSON")?;
        let fetched_at = now_ts();

        let mut conn = self.conn.lock().unwrap();
        conn.execute(
            "
            INSERT INTO user_activity_snapshots (user_address, fetched_at, raw_json)
            VALUES (?1, ?2, ?3)
            ",
            params![user_address, fetched_at, raw_json],
        )
        .context("Failed to insert user activity snapshot")?;

        Ok(())
    }
}

fn open(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open SQLite DB at {}", db_path))?;

    conn.busy_timeout(Duration::from_secs(5))
        .context("Failed to configure SQLite busy timeout")?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("Failed to enable SQLite WAL journal mode")?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .context("Failed to configure SQLite synchronous mode")?;

    Ok(conn)
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
