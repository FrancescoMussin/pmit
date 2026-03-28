#!/usr/bin/env python3
"""Import training_samples rows from CSV into training.db."""

import argparse
import csv
import sqlite3
import sys
from pathlib import Path


def parse_exposure(raw: str):
    value = (raw or "").strip()
    if value == "" or value.upper() == "NULL":
        return None
    return float(value)


def ensure_table(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS training_samples (
            title TEXT,
            outcome TEXT,
            exposure REAL
        )
        """
    )


def upsert_row_preserving_progress(
    conn: sqlite3.Connection,
    title: str | None,
    outcome: str | None,
    exposure: float | None,
) -> tuple[int, int]:
    """Merge one CSV row into training_samples without deleting existing labels.

    Returns (inserted, updated).
    """
    existing = conn.execute(
        """
        SELECT rowid, exposure
        FROM training_samples
        WHERE title IS ? AND outcome IS ?
        LIMIT 1
        """,
        (title, outcome),
    ).fetchone()

    if existing is None:
        conn.execute(
            "INSERT INTO training_samples (title, outcome, exposure) VALUES (?, ?, ?)",
            (title, outcome, exposure),
        )
        return (1, 0)

    rowid, current_exposure = existing
    if current_exposure is None and exposure is not None:
        conn.execute(
            "UPDATE training_samples SET exposure = ? WHERE rowid = ?",
            (exposure, rowid),
        )
        return (0, 1)

    return (0, 0)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Import CSV rows into training_samples table"
    )
    parser.add_argument(
        "--db",
        default="./databases/training.db",
        help="Path to training SQLite DB (default: ./databases/training.db)",
    )
    parser.add_argument(
        "--in",
        dest="input_csv",
        default="./training_samples.csv",
        help="Input CSV path (default: ./training_samples.csv)",
    )
    parser.add_argument(
        "--replace",
        action="store_true",
        help="Replace table contents before import (destructive)",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    input_path = Path(args.input_csv)

    if not input_path.exists():
        print(f"Input CSV not found: {input_path}", file=sys.stderr)
        return 1

    db_path.parent.mkdir(parents=True, exist_ok=True)

    with input_path.open("r", newline="", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        expected = {"title", "outcome", "exposure"}
        if not reader.fieldnames or set(reader.fieldnames) != expected:
            print(
                "CSV header must contain exactly: title,outcome,exposure",
                file=sys.stderr,
            )
            return 1

        rows = []
        for row in reader:
            try:
                rows.append(
                    (
                        row.get("title"),
                        row.get("outcome"),
                        parse_exposure(row.get("exposure", "")),
                    )
                )
            except ValueError as exc:
                print(f"Invalid exposure value in row {row}: {exc}", file=sys.stderr)
                return 1

    conn = sqlite3.connect(str(db_path))
    try:
        ensure_table(conn)

        inserted = 0
        updated = 0

        if args.replace:
            conn.execute("DELETE FROM training_samples")
            conn.executemany(
                "INSERT INTO training_samples (title, outcome, exposure) VALUES (?, ?, ?)",
                rows,
            )
            inserted = len(rows)
        else:
            for title, outcome, exposure in rows:
                i, u = upsert_row_preserving_progress(conn, title, outcome, exposure)
                inserted += i
                updated += u

        conn.commit()

        total = conn.execute("SELECT COUNT(*) FROM training_samples").fetchone()[0]
    finally:
        conn.close()

    print(f"Imported {len(rows)} rows from {input_path}")
    if args.replace:
        print("Mode: replace (destructive)")
    else:
        print("Mode: merge (preserve existing progress)")
    print(f"Rows inserted: {inserted}")
    print(f"Rows updated from NULL exposure: {updated}")
    print(f"Total rows in training_samples: {total}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
