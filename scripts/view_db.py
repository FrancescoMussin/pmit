#!/usr/bin/env python3
import argparse
import json
import sqlite3
import sys
from datetime import UTC, datetime
from pathlib import Path
from textwrap import shorten


def fetch_tables(conn: sqlite3.Connection) -> list[str]:
    rows = conn.execute(
        """
        SELECT name
        FROM sqlite_master
        WHERE type='table' AND name NOT LIKE 'sqlite_%'
        ORDER BY name
        """
    ).fetchall()
    return [row[0] for row in rows]


def fetch_rows(conn: sqlite3.Connection, table: str, limit: int) -> list[sqlite3.Row]:
    query = f"SELECT * FROM {table} ORDER BY rowid DESC LIMIT ?"
    return conn.execute(query, (limit,)).fetchall()


def shorten_id(value: str, head: int = 8, tail: int = 6) -> str:
    if len(value) <= head + tail + 3:
        return value
    return f"{value[:head]}...{value[-tail:]}"


def format_epoch(value: int) -> str:
    return datetime.fromtimestamp(value, tz=UTC).strftime("%Y-%m-%d %H:%M:%S UTC")


def summarize_activity(raw_json: str) -> str:
    try:
        payload = json.loads(raw_json)
    except json.JSONDecodeError:
        return "invalid json"

    if isinstance(payload, list):
        return f"{len(payload)} events"
    if isinstance(payload, dict):
        return f"object with {len(payload)} keys"
    return type(payload).__name__


def make_human_view(table: str, rows: list[sqlite3.Row]) -> tuple[list[str], list[dict[str, str]]]:
    if table == "trades":
        columns = [
            "trade_time",
            "ingested_at",
            "title",
            "outcome",
            "side",
            "size",
            "price",
            "maker",
            "tx_hash",
        ]
        formatted_rows = []
        for row in rows:
            formatted_rows.append(
                {
                    "trade_time": format_epoch(int(row["timestamp"])),
                    "ingested_at": format_epoch(int(row["ingested_at"])),
                    "title": row["title"] or "Unknown Market",
                    "outcome": row["outcome"] or "N/A",
                    "side": row["side"],
                    "size": f"{float(row['size']):.4f}",
                    "price": f"${float(row['price']):.4f}",
                    "maker": shorten_id(str(row["maker_address"])),
                    "tx_hash": shorten_id(str(row["transaction_hash"])),
                }
            )
        return columns, formatted_rows

    if table == "user_activity_snapshots":
        columns = ["fetched_at", "user", "summary"]
        formatted_rows = []
        for row in rows:
            formatted_rows.append(
                {
                    "fetched_at": format_epoch(int(row["fetched_at"])),
                    "user": shorten_id(str(row["user_address"])),
                    "summary": summarize_activity(str(row["raw_json"])),
                }
            )
        return columns, formatted_rows

    if not rows:
        return [], []

    columns = [col for col in rows[0].keys() if col != "raw_json"]
    formatted_rows = []
    for row in rows:
        rendered = {}
        for col in columns:
            value = row[col]
            rendered[col] = "NULL" if value is None else str(value)
        formatted_rows.append(rendered)
    return columns, formatted_rows


def render_table(
    title: str,
    columns: list[str],
    rows: list[dict[str, str]],
    max_col_width: int,
) -> str:
    if not columns:
        return f"\n== {title} ==\n(no columns found)"

    display_rows: list[list[str]] = []
    widths = {col: len(col) for col in columns}

    for row in rows:
        rendered_row: list[str] = []
        for col in columns:
            value = row[col]
            text = str(value).replace("\n", " ")
            text = shorten(text, width=max_col_width, placeholder="…")
            rendered_row.append(text)
            widths[col] = min(max_col_width, max(widths[col], len(text)))
        display_rows.append(rendered_row)

    line_top = "=" * (sum(widths.values()) + 3 * (len(columns) - 1))
    header = " | ".join(col.ljust(widths[col]) for col in columns)
    separator = "-+-".join("-" * widths[col] for col in columns)

    output = [f"\n== {title} ==", line_top, header, separator]
    if not display_rows:
        output.append("(no rows)")
    else:
        for rendered_row in display_rows:
            output.append(
                " | ".join(rendered_row[i].ljust(widths[columns[i]]) for i in range(len(columns)))
            )
    return "\n".join(output)


def main() -> int:
    parser = argparse.ArgumentParser(description="View SQLite tables from PMIT database")
    parser.add_argument(
        "--db",
        default="./databases/trades.db",
        help="Path to SQLite DB file (default: ./databases/trades.db)",
    )
    parser.add_argument(
        "--table",
        default="all",
        help="Table to print (default: all non-system tables)",
    )
    parser.add_argument("--limit", type=int, default=20, help="Rows per table (default: 20)")
    parser.add_argument(
        "--max-col-width",
        type=int,
        default=90,
        help="Max characters per cell before truncation (default: 90)",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    if args.limit < 1:
        print("--limit must be >= 1", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row

    try:
        tables = fetch_tables(conn)
        if not tables:
            print("No tables found in database.")
            return 0

        if args.table == "all":
            selected_tables = tables
        else:
            if args.table not in tables:
                print(
                    f"Table '{args.table}' not found. Available: {', '.join(tables)}",
                    file=sys.stderr,
                )
                return 1
            selected_tables = [args.table]

        for table in selected_tables:
            rows = fetch_rows(conn, table, args.limit)
            columns, human_rows = make_human_view(table, rows)
            print(render_table(table, columns, human_rows, args.max_col_width))

    finally:
        conn.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
