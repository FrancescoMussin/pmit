#!/usr/bin/env python3
"""Export training_samples rows from training.db to CSV."""

import argparse
import csv
import sqlite3
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Export training_samples table to CSV"
    )
    parser.add_argument(
        "--db",
        default="./databases/training.db",
        help="Path to training SQLite DB (default: ./databases/training.db)",
    )
    parser.add_argument(
        "--out",
        default="./training_samples.csv",
        help="Output CSV path (default: ./training_samples.csv)",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    out_path = Path(args.out)

    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        cur = conn.cursor()
        rows = cur.execute(
            "SELECT title, outcome, exposure FROM training_samples"
        ).fetchall()
    finally:
        conn.close()

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow(["title", "outcome", "exposure"])
        writer.writerows(rows)

    print(f"Exported {len(rows)} rows to {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
