use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::polymarket::Trade;

/// SQLite gateway for trades persistence.
#[derive(Debug, Clone)]
pub struct TradeDatabaseHandler {
    db_path: String,
}

impl TradeDatabaseHandler {
    /// Create a new handler bound to a database file path.
    pub fn new(db_path: String) -> Self {
        Self { db_path }
    }

    /// Return the configured SQLite file path.
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Initialize schema and indexes for the trades database.
    pub fn init_schema(&self) -> Result<()> {
        with_conn(self.db_path(), |conn| {
            conn.execute_batch(
                "
                -- We create the table if it doesn't exist already.
                CREATE TABLE IF NOT EXISTS trades (
                    -- Unique identifier for each trade, using the transaction hash from Polymarket
                    transaction_hash TEXT PRIMARY KEY,
                    -- The wallet address of the user who made the trade
                    maker_address TEXT NOT NULL,
                    -- The side of the trade, either 'buy' or 'sell'
                    side TEXT NOT NULL,
                    -- This is a token/contract ID representing the specific market outcome that was traded.
                    asset TEXT NOT NULL,
                    -- The title of the market for this trade
                    title TEXT,
                    -- The specific outcome that was traded on, if applicable
                    outcome TEXT,
                    -- The size of the trade in shares
                    size REAL NOT NULL,
                    -- The price per share for the trade
                    price REAL NOT NULL,
                    -- The timestamp of when the trade occurred, in Unix time
                    timestamp INTEGER NOT NULL,
                    -- The timestamp of when this trade was ingested into our database, in Unix time
                    ingested_at INTEGER NOT NULL
                );

                -- this creates a B-tree index on the timestamp column to speed up queries on the timestamp
                CREATE INDEX IF NOT EXISTS idx_trades_timestamp ON trades(timestamp);
                -- this creates a B-tree index on the maker_address column to speed up queries filtering by user address
                CREATE INDEX IF NOT EXISTS idx_trades_maker_address ON trades(maker_address);
                ",
            )
            .context("Failed to initialize SQLite schema")?;

            Ok(())
        })
    }

    /// Insert one trade snapshot.
    ///
    /// Uses `INSERT OR IGNORE` on transaction hash to keep inserts idempotent.
    pub fn insert_trade(&self, trade: &Trade) -> Result<()> {
        with_conn(self.db_path(), |conn| {
            let ingested_at = now_ts();

            conn.execute(
                "
                INSERT OR IGNORE INTO trades (
                    transaction_hash, maker_address, side, asset, title, outcome,
                    size, price, timestamp, ingested_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ",
                params![
                    trade.transaction_hash,
                    trade.maker_address.as_str(),
                    trade.side.to_string(),
                    trade.asset.as_str(),
                    trade.title,
                    trade.outcome,
                    trade.size,
                    trade.price,
                    trade.timestamp,
                    ingested_at,
                ],
            )
            .context("Failed to insert trade")?;

            Ok(())
        })
    }
}

/// SQLite gateway for user activity snapshots.
#[derive(Debug, Clone)]
pub struct UserHistoryDatabaseHandler {
    db_path: String,
}

impl UserHistoryDatabaseHandler {
    pub fn new(db_path: String) -> Self {
        Self { db_path }
    }

    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Initialize schema and indexes for user history snapshots.
    pub fn init_schema(&self) -> Result<()> {
        with_conn(self.db_path(), |conn| {
            conn.execute_batch(
                "
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
            .context("Failed to initialize user history schema")?;

            Ok(())
        })
    }

    /// Insert one user-activity snapshot captured during enrichment.
    pub fn insert_user_activity_snapshot(
        &self,
        user_address: &str,
        activity: &[Value],
    ) -> Result<()> {
        with_conn(self.db_path(), |conn| {
            let raw_json = serde_json::to_string(activity)
                .context("Failed to serialize user activity snapshot JSON")?;
            let fetched_at = now_ts();

            conn.execute(
                "
                INSERT INTO user_activity_snapshots (user_address, fetched_at, raw_json)
                VALUES (?1, ?2, ?3)
                ",
                params![user_address, fetched_at, raw_json],
            )
            .context("Failed to insert user activity snapshot")?;

            Ok(())
        })
    }
}

/// Ensure a SQLite database file exists and is openable.
///
/// Useful for DBs managed outside this Rust service (for example, Python-side
/// training workflows) where we still want startup validation.
pub fn ensure_database_file(db_path: &str) -> Result<()> {
    with_conn(db_path, |_conn| Ok(()))
}

/// Open/configure a connection and execute one database operation.
fn with_conn<T, F>(db_path: &str, f: F) -> Result<T>
where
    F: FnOnce(&Connection) -> Result<T>,
{
    let conn = open(db_path)?;
    f(&conn)
}

/// Open and configure a SQLite connection for concurrent read/write workload.
fn open(db_path: &str) -> Result<Connection> {
    ensure_parent_dir_exists(db_path)?;

    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open SQLite DB at {}", db_path))?;

    // We set things up so that when we query the database and this is blocked by another connection writing to it,
    // instead of immediately returning an error it will wait for a bit and retry, which helps with concurrency.
    conn.busy_timeout(Duration::from_secs(5))
        .context("Failed to configure SQLite busy timeout")?;
    // We set things up so that when we write to the database, we first write to a separate WAL file and then later
    // merge it with the main database file, which allows for better performance and concurrency.
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("Failed to enable SQLite WAL journal mode")?;
    // We set things up so that the database is synchronized in a way that balances performance and data safety.
    conn.pragma_update(None, "synchronous", "NORMAL")
        .context("Failed to configure SQLite synchronous mode")?;

    Ok(conn)
}

fn ensure_parent_dir_exists(db_path: &str) -> Result<()> {
    let path = Path::new(db_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create database directory {:?}", parent))?;
        }
    }
    Ok(())
}

/// Current unix timestamp in seconds.
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
