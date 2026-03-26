use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::polymarket::Trade;

pub fn init(db_path: &str) -> Result<()> {
    // we open a connection to the database so that rust can talk to it.
    let conn = open(db_path)?;
    // this could be improved by keeping a single connection open and reusing it,
    // but when we tried that it was actually slower.

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
            -- The raw JSON data of the trade as returned by the Polymarket API, stored for reference and potential future use
            raw_json TEXT NOT NULL,
            -- The timestamp of when this trade was ingested into our database, in Unix time
            ingested_at INTEGER NOT NULL
        );

        -- this creates a B-tree index on the timestamp column to speed up queries on the timestamp
        CREATE INDEX IF NOT EXISTS idx_trades_timestamp ON trades(timestamp);
        -- this creates a B-tree index on the maker_address column to speed up queries filtering by user address
        CREATE INDEX IF NOT EXISTS idx_trades_maker_address ON trades(maker_address);

        -- We also create a table to store snapshots of user activity
        CREATE TABLE IF NOT EXISTS user_activity_snapshots (
            -- we assign surrogate ids automatically to each snapshot for easier referencing
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            -- the wallet address of the user whose activity is being stored
            user_address TEXT NOT NULL,
            -- the timestamp of when this snapshot was fetched from the Polymarket API, in Unix time
            fetched_at INTEGER NOT NULL,
            -- the raw JSON data of the user's activity as returned by the Polymarket API, stored for reference and potential future use
            raw_json TEXT NOT NULL
        );

        -- this creates a B-tree index on the user_address and fetched_at columns to speed up queries filtering by user and time
        CREATE INDEX IF NOT EXISTS idx_user_activity_user_time
            ON user_activity_snapshots(user_address, fetched_at);
        ",
    )
    .context("Failed to initialize SQLite schema")?;

    Ok(())
}

/// Inserts a trade into the database.
pub fn insert_trade(db_path: &str, trade: &Trade) -> Result<()> {
    // open up the connection to the database
    let conn = open(db_path)?;
    // we get the raw_json of the trade.
    let raw_json = serde_json::to_string(trade).context("Failed to serialize trade JSON")?;
    // we also get the time the trade was ingested.
    let ingested_at = now_ts();

    // we insert the trade into the database.
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

/// Inserts a user activity snapshot into the database.
pub fn insert_user_activity_snapshot(
    db_path: &str,
    user_address: &str,
    activity: &[Value],
) -> Result<()> {
    // open up the connection to the database
    let conn = open(db_path)?;
    // we get the raw_json of the user activity snapshot.
    let raw_json = serde_json::to_string(activity)
        .context("Failed to serialize user activity snapshot JSON")?;
    // we get the time this was ingested.
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
}

/// Opens a connection to the SQLite database at the specified path, with appropriate configuration for concurrent access and performance.
fn open(db_path: &str) -> Result<Connection> {
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

/// Helper function to get the current timestamp in seconds since the Unix epoch.
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
