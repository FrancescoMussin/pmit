#!/usr/bin/env python3
"""Interactive manual labeler for exposure values in training_samples.

Usage example:
    python3 scripts/manual_label_exposure.py --db ./databases/training.db --limit 300

Controls while labeling:
- 1: label as high exposure (1.0)
- 0: label as low exposure (0.0)
- any decimal in [0, 1]: set custom exposure (example: 0.35)
- s: skip current row
- q: quit and save progress
"""

import argparse
import sqlite3
import sys
from pathlib import Path


def ensure_exposure_column(conn: sqlite3.Connection, table: str) -> None:
    cols = {row[1] for row in conn.execute(f"PRAGMA table_info({table})").fetchall()}
    if "exposure" not in cols:
        conn.execute(f"ALTER TABLE {table} ADD COLUMN exposure REAL")
        conn.commit()


def fetch_rows(conn: sqlite3.Connection, table: str, limit: int) -> list[sqlite3.Row]:
    conn.row_factory = sqlite3.Row
    return conn.execute(
        f"""
        SELECT rowid, title, outcome
        FROM {table}
        WHERE exposure IS NULL
        ORDER BY RANDOM()
        LIMIT ?
        """,
        (limit,),
    ).fetchall()


def set_exposure(conn: sqlite3.Connection, table: str, rowid: int, value: float) -> None:
    conn.execute(
        f"UPDATE {table} SET exposure = ? WHERE rowid = ?",
        (value, rowid),
    )


def count_labels(conn: sqlite3.Connection, table: str) -> tuple[int, int]:
    labeled = conn.execute(
        f"SELECT COUNT(*) FROM {table} WHERE exposure IS NOT NULL"
    ).fetchone()[0]
    unlabeled = conn.execute(
        f"SELECT COUNT(*) FROM {table} WHERE exposure IS NULL"
    ).fetchone()[0]
    return labeled, unlabeled


def normalize_choice(raw: str) -> str:
    return (raw or "").strip().lower()


def parse_label_value(choice: str):
    if choice == "1":
        return 1.0
    if choice == "0":
        return 0.0

    try:
        value = float(choice)
    except ValueError:
        return None

    if 0.0 <= value <= 1.0:
        return value

    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Manually label exposure for training samples")
    parser.add_argument(
        "--db",
        default="./databases/training.db",
        help="Path to SQLite DB file (default: ./databases/training.db)",
    )
    parser.add_argument(
        "--table",
        default="training_samples",
        help="Table to label (default: training_samples)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=300,
        help="How many unlabeled rows to load this session (default: 300)",
    )
    parser.add_argument(
        "--commit-every",
        type=int,
        default=20,
        help="Commit every N applied labels (default: 20)",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        ensure_exposure_column(conn, args.table)

        rows = fetch_rows(conn, args.table, args.limit)
        if not rows:
            print("No unlabeled rows found. Nothing to do.")
            return 0

        print(f"Loaded {len(rows)} unlabeled rows from {args.table}.")
        print(
            "Commands: [1]=high, [0]=low, [0..1]=custom score, [s]=skip, [q]=quit"
        )

        labeled_this_session = 0
        skipped = 0
        dirty_since_commit = 0

        for i, row in enumerate(rows, start=1):
            title = row["title"] if row["title"] is not None else ""
            outcome = row["outcome"] if row["outcome"] is not None else ""

            print("\n" + "=" * 80)
            print(f"[{i}/{len(rows)}]")
            print(f"title:   {title}")
            print(f"outcome: {outcome}")

            while True:
                choice = normalize_choice(input("label (1/0/s/q): "))

                value = parse_label_value(choice)
                if value is not None:
                    set_exposure(conn, args.table, row["rowid"], value)
                    labeled_this_session += 1
                    dirty_since_commit += 1
                    break

                if choice == "s":
                    skipped += 1
                    break

                if choice == "q":
                    conn.commit()
                    total_labeled, total_unlabeled = count_labels(conn, args.table)
                    print("\nSession ended early.")
                    print(f"Applied labels this session: {labeled_this_session}")
                    print(f"Skipped this session: {skipped}")
                    print(f"Table totals -> labeled: {total_labeled}, unlabeled: {total_unlabeled}")
                    return 0

                print("Invalid input. Use 1, 0, a decimal between 0 and 1, s, or q.")

            if dirty_since_commit >= args.commit_every:
                conn.commit()
                dirty_since_commit = 0

        conn.commit()
        total_labeled, total_unlabeled = count_labels(conn, args.table)

        print("\nSession complete.")
        print(f"Applied labels this session: {labeled_this_session}")
        print(f"Skipped this session: {skipped}")
        print(f"Table totals -> labeled: {total_labeled}, unlabeled: {total_unlabeled}")
        return 0

    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
