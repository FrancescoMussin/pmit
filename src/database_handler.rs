use anyhow::{Context, Result, anyhow};
use rusqlite::params;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_rusqlite::Connection;

use crate::polymarket::Trade;

/// SQLite gateway for trades persistence.
#[derive(Debug, Clone)]
pub struct TradeDatabaseHandler {
    // The file path to the SQLite database for trades.
    db_path: String,
    // The asynchronous SQLite connection handle.
    conn: Connection,
}

impl TradeDatabaseHandler {
    /// Create a new handler bound to a database file path.
    pub async fn new(db_path: String) -> Result<Self> {
        let conn = open(&db_path).await?;
        Ok(Self { db_path, conn })
    }

    /// Return the configured SQLite file path.
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Initialize schema and indexes for the trades database.
    pub async fn init_schema(&self) -> Result<()> {
        self.conn.call(|conn| {
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
            // for standardizing error types
            Ok::<(), anyhow::Error>(())
        })
        .await
        .map_err(|e| anyhow!(e))
        .context("Failed to call init_schema on background thread")
    }

    /// Insert one trade snapshot.
    ///
    /// Uses `INSERT OR IGNORE` on transaction hash to keep inserts idempotent.
    pub async fn insert_trade(&self, trade: Trade) -> Result<()> {
        let ingested_at = now_ts();
        self.conn
            .call(move |conn| {
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

                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call insert_trade on background thread")
    }

    /// Insert a batch of trades using a single transaction.
    pub async fn insert_trades_batch(&self, trades: Vec<Trade>) -> Result<()> {
        let ingested_at = now_ts();
        // We use `move` to transfer ownership of the owned `trades` (Vec<Trade>) into the closure.
        // This is required because the closure is sent to a background thread and must own its data
        // to ensure it stays valid.
        self.conn
            .call(move |conn| {
                let tx = conn.transaction().context("Failed to start transaction")?;
                {
                    let mut stmt = tx.prepare_cached(
                        "
                    INSERT OR IGNORE INTO trades (
                        transaction_hash, maker_address, side, asset, title, outcome,
                        size, price, timestamp, ingested_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                    ",
                    )?;

                    for trade in trades {
                        stmt.execute(params![
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
                        ])?;
                    }
                }
                tx.commit().context("Failed to commit transaction")?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call insert_trades_batch on background thread")
    }
}

#[derive(Debug, Clone)]
pub struct UserHistoryDatabaseHandler {
    db_path: String,
    conn: Connection,
}

impl UserHistoryDatabaseHandler {
    /// Create a new handler bound to a database file path.
    pub async fn new(db_path: String) -> Result<Self> {
        let conn = open(&db_path).await?;
        Ok(Self { db_path, conn })
    }

    /// Return the configured SQLite file path.
    pub fn db_path(&self) -> &str {
        &self.db_path
    }

    /// Initialize schema and indexes for user history snapshots.
    pub async fn init_schema(&self) -> Result<()> {
        self.conn
            .call(|conn| {
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

                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call init_schema for user history")
    }

    pub async fn insert_user_activity_snapshot(
        &self,
        user_address: String,
        activity: Vec<Value>,
    ) -> Result<()> {
        // We use `move` to transfer ownership of the owned `user_address` (String) and
        // `activity` (Vec<Value>) into the closure. This is required because the closure is
        // sent to a background thread and must own its data to ensure it stays valid.
        self.conn
            .call(move |conn| {
                let raw_json = serde_json::to_string(&activity)
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

                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call insert_user_activity_snapshot")
    }

    /// Checkpoint and truncate WAL for this database.
    pub async fn checkpoint_truncate(&self) -> Result<()> {
        self.conn
            .call(|conn| {
                conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .context("Failed to checkpoint user history database WAL")?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call checkpoint_truncate for user history")
    }
}

impl TradeDatabaseHandler {
    /// Checkpoint and truncate WAL for this database.
    pub async fn checkpoint_truncate(&self) -> Result<()> {
        self.conn
            .call(|conn| {
                conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .context("Failed to checkpoint trades database WAL")?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!(e))
            .context("Failed to call checkpoint_truncate for trades")
    }
}

/// Ensure a SQLite database file exists and is openable.
///
/// Useful for DBs managed outside this Rust service (for example, Python-side
/// training workflows) where we still want startup validation.
pub async fn ensure_database_file(db_path: &str) -> Result<()> {
    let _conn = open(db_path).await?;
    Ok(())
}

/// Checkpoint and truncate WAL for a database path not owned by a handler.
pub async fn checkpoint_database_file(db_path: &str) -> Result<()> {
    let conn = open(db_path).await?;
    conn.call(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .context("Failed to checkpoint database WAL")?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow!(e))
    .context("Failed to call checkpoint on database file")
}

/// Open and configure a SQLite connection for concurrent read/write workload.
async fn open(db_path: &str) -> Result<Connection> {
    ensure_parent_dir_exists(db_path)?;

    let conn = Connection::open(db_path)
        .await
        .with_context(|| format!("Failed to open SQLite DB at {}", db_path))?;

    conn.call(|conn| {
        // We set things up so that when we query the database and this is blocked by another
        // connection writing to it, instead of immediately returning an error it will wait
        // for a bit and retry, which helps with concurrency.
        conn.busy_timeout(Duration::from_secs(5))
            .context("Failed to configure SQLite busy timeout")?;
        // We set things up so that when we write to the database, we first write to a separate WAL
        // file and then later merge it with the main database file, which allows for better
        // performance and concurrency.
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("Failed to enable SQLite WAL journal mode")?;
        // We set things up so that the database is synchronized in a way that balances performance
        // and data safety.
        conn.pragma_update(None, "synchronous", "NORMAL")
            .context("Failed to configure SQLite synchronous mode")?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow!(e))
    .context("Failed to configure SQLite connection")?;

    Ok(conn)
}

/// Ensure the parent directory for a database file exists, creating it if needed.
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
