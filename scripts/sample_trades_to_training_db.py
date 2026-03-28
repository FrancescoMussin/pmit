#!/usr/bin/env python3
"""Copy a random sample of trades from trades.db into training.db.

By default this script replaces rows in `training_samples` with a fresh random
sample from `trades`.
"""

import argparse
import sqlite3
import sys
from pathlib import Path


def ensure_training_table(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS training_samples (
            title TEXT,
            outcome TEXT
        )
        """
    )


def fetch_random_trades(conn: sqlite3.Connection, sample_size: int) -> list[sqlite3.Row]:
    conn.row_factory = sqlite3.Row
    return conn.execute(
        """
        SELECT
            title,
            outcome
        FROM trades
        ORDER BY RANDOM()
        LIMIT ?
        """,
        (sample_size,),
    ).fetchall()


def insert_samples(conn: sqlite3.Connection, rows: list[sqlite3.Row]) -> int:
    cur = conn.executemany(
        """
        INSERT OR REPLACE INTO training_samples (
            title,
            outcome
        ) VALUES (?, ?)
        """,
        [
            (
                row["title"],
                row["outcome"],
            )
            for row in rows
        ],
    )
    return cur.rowcount if cur.rowcount is not None else 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Copy random trades sample from trades DB into training DB"
    )
    parser.add_argument(
        "--source-db",
        default="./databases/trades.db",
        help="Path to source trades database (default: ./databases/trades.db)",
    )
    parser.add_argument(
        "--target-db",
        default="./databases/training.db",
        help="Path to target training database (default: ./databases/training.db)",
    )
    parser.add_argument(
        "--sample-size",
        type=int,
        default=500,
        help="Number of random rows to copy (default: 500)",
    )
    parser.add_argument(
        "--append",
        action="store_true",
        help="Append to existing training_samples instead of replacing it",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show planned actions without writing to target DB",
    )
    args = parser.parse_args()

    source_path = Path(args.source_db)
    target_path = Path(args.target_db)

    if not source_path.exists():
        print(f"Source DB not found: {source_path}", file=sys.stderr)
        return 1

    target_path.parent.mkdir(parents=True, exist_ok=True)

    source_conn = sqlite3.connect(str(source_path))
    target_conn = sqlite3.connect(str(target_path))

    try:
        rows = fetch_random_trades(source_conn, args.sample_size)

        print(f"Source DB: {source_path}")
        print(f"Target DB: {target_path}")
        print(f"Requested sample size: {args.sample_size}")
        print(f"Rows sampled: {len(rows)}")

        if args.dry_run:
            print("Dry run complete: no rows written.")
            return 0

        ensure_training_table(target_conn)

        if not args.append:
            target_conn.execute("DELETE FROM training_samples")

        inserted = insert_samples(target_conn, rows)
        target_conn.commit()

        total_training_rows = target_conn.execute(
            "SELECT COUNT(*) FROM training_samples"
        ).fetchone()[0]

        print(f"Rows written this run: {inserted}")
        print(f"Total rows in training_samples: {total_training_rows}")
        return 0
    finally:
        source_conn.close()
        target_conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
