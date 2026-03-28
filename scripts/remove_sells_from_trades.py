#!/usr/bin/env python3
"""Delete sell-side rows from the `trades` table.

By default this script runs in dry-run mode and reports how many rows would be
removed. Use `--apply` to execute the deletion.
"""

import argparse
import sqlite3
import sys
from pathlib import Path


def count_rows(conn: sqlite3.Connection) -> int:
    row = conn.execute("SELECT COUNT(*) FROM trades").fetchone()
    return int(row[0]) if row else 0


def count_sell_rows(conn: sqlite3.Connection) -> int:
    row = conn.execute(
        "SELECT COUNT(*) FROM trades WHERE lower(side) = 'sell'"
    ).fetchone()
    return int(row[0]) if row else 0


def delete_sell_rows(conn: sqlite3.Connection) -> int:
    conn.execute("BEGIN")
    try:
        cur = conn.execute("DELETE FROM trades WHERE lower(side) = 'sell'")
        deleted = cur.rowcount if cur.rowcount is not None else 0
        conn.execute("COMMIT")
        return deleted
    except Exception:
        conn.execute("ROLLBACK")
        raise


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Delete sell-side rows from trades table"
    )
    parser.add_argument(
        "--db",
        default="./databases/trades.db",
        help="Path to SQLite DB file (default: ./databases/trades.db)",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Apply deletion. Without this flag, script only reports what would be deleted.",
    )
    parser.add_argument(
        "--vacuum",
        action="store_true",
        help="Run VACUUM after deletion (only with --apply).",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        total_before = count_rows(conn)
        sells_before = count_sell_rows(conn)

        if not args.apply:
            print(f"Dry run: would delete {sells_before} sell rows from trades.")
            print(f"Total rows before: {total_before}")
            print(f"Total rows after:  {total_before - sells_before}")
            return 0

        deleted = delete_sell_rows(conn)
        total_after = count_rows(conn)

        print(f"Deleted {deleted} sell rows from trades.")
        print(f"Total rows before: {total_before}")
        print(f"Total rows after:  {total_after}")

        if args.vacuum:
            print("Running VACUUM...")
            conn.execute("VACUUM")
            print("VACUUM complete.")

        return 0
    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
