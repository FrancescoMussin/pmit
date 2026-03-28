#!/usr/bin/env python3
"""Remove the `raw_json` column from the `trades` table.

This migration is safe to run multiple times:
- If `trades.raw_json` is already absent, it exits without changes.
- If present, it recreates `trades` without that column and preserves data.
"""

import argparse
import sqlite3
import sys
from pathlib import Path


def column_exists(conn: sqlite3.Connection, table: str, column: str) -> bool:
    cur = conn.execute(f"PRAGMA table_info({table})")
    return any(row[1] == column for row in cur.fetchall())


def migrate_drop_raw_json(conn: sqlite3.Connection) -> None:
    conn.execute("BEGIN")
    try:
        conn.execute("ALTER TABLE trades RENAME TO trades_old")

        conn.execute(
            """
            CREATE TABLE trades (
                transaction_hash TEXT PRIMARY KEY,
                maker_address TEXT NOT NULL,
                side TEXT NOT NULL,
                asset TEXT NOT NULL,
                title TEXT,
                outcome TEXT,
                size REAL NOT NULL,
                price REAL NOT NULL,
                timestamp INTEGER NOT NULL,
                ingested_at INTEGER NOT NULL
            )
            """
        )

        conn.execute(
            """
            INSERT INTO trades (
                transaction_hash, maker_address, side, asset, title, outcome,
                size, price, timestamp, ingested_at
            )
            SELECT
                transaction_hash, maker_address, side, asset, title, outcome,
                size, price, timestamp, ingested_at
            FROM trades_old
            """
        )

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trades_timestamp ON trades(timestamp)"
        )
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trades_maker_address ON trades(maker_address)"
        )

        conn.execute("DROP TABLE trades_old")
        conn.execute("COMMIT")
    except Exception:
        conn.execute("ROLLBACK")
        raise


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Remove raw_json column from trades table"
    )
    parser.add_argument(
        "--db",
        default="./databases/trades.db",
        help="Path to SQLite DB (default: ./databases/trades.db)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Report what would happen without modifying the DB",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        if not column_exists(conn, "trades", "raw_json"):
            print("No changes: trades.raw_json does not exist.")
            return 0

        if args.dry_run:
            print("Would remove column: trades.raw_json")
            return 0

        migrate_drop_raw_json(conn)
        print("Migration complete: removed trades.raw_json")
        return 0
    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
