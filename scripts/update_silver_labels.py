#!/usr/bin/env python3
import argparse
import sqlite3
import sys
from pathlib import Path


def ensure_column(conn: sqlite3.Connection) -> bool:
    cur = conn.cursor()
    cur.execute("PRAGMA table_info(trades)")
    cols = {r[1] for r in cur.fetchall()}

    if "silver_label" in cols:
        return False

    cur.execute("ALTER TABLE trades ADD COLUMN silver_label REAL")
    return True


def apply_labels(conn: sqlite3.Connection, overwrite: bool) -> int:
    where_clause = "" if overwrite else "WHERE silver_label IS NULL"

    query = f"""
        UPDATE trades
        SET silver_label = CASE
            WHEN lower(COALESCE(title, '')) LIKE '%fed%'
              OR lower(COALESCE(title, '')) LIKE '%interest rates%'
              OR lower(COALESCE(title, '')) LIKE '%fda%'
              OR lower(COALESCE(title, '')) LIKE '%sec%'
              OR lower(COALESCE(title, '')) LIKE '%approval%'
            THEN 0.75
            WHEN lower(COALESCE(title, '')) LIKE '%weather%'
              OR lower(COALESCE(title, '')) LIKE '%sports%'
              OR lower(COALESCE(title, '')) LIKE '%friendly%'
            THEN 0.35
            When lower(COALESCE(title, '')) LIKE '%Up or Down%'
              OR lower(COALESCE(title, '')) LIKE '%Will it%'
            THEN 0.1
            ELSE 0.50
        END
        {where_clause}
    """

    cur = conn.cursor()
    cur.execute(query)
    return cur.rowcount


def summarize(conn: sqlite3.Connection) -> tuple[int, int, int]:
    cur = conn.cursor()
    total = cur.execute("SELECT COUNT(*) FROM trades").fetchone()[0]
    labeled = cur.execute(
        "SELECT COUNT(*) FROM trades WHERE silver_label IS NOT NULL"
    ).fetchone()[0]
    unlabeled = total - labeled
    return total, labeled, unlabeled


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Update PMIT SQLite DB with silver_label values"
    )
    parser.add_argument(
        "--db",
        default="pmit.db",
        help="Path to SQLite DB file (default: pmit.db)",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Recompute labels for all rows (default only fills NULL labels)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print intended actions without modifying the database",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        if args.dry_run:
            cur = conn.cursor()
            cur.execute("PRAGMA table_info(trades)")
            cols = {r[1] for r in cur.fetchall()}
            has_column = "silver_label" in cols
            total, labeled, unlabeled = summarize(conn)
            print(f"DB: {db_path}")
            print(f"silver_label column exists: {has_column}")
            print(
                "Would update: "
                + ("all rows" if args.overwrite else "only rows where silver_label IS NULL")
            )
            print(f"Current rows -> total: {total}, labeled: {labeled}, unlabeled: {unlabeled}")
            return 0

        column_added = ensure_column(conn)
        updated_rows = apply_labels(conn, overwrite=args.overwrite)
        conn.commit()

        total, labeled, unlabeled = summarize(conn)
        print(f"DB: {db_path}")
        print(f"silver_label column added: {column_added}")
        print(f"Rows updated: {updated_rows}")
        print(f"Rows summary -> total: {total}, labeled: {labeled}, unlabeled: {unlabeled}")

    finally:
        conn.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
