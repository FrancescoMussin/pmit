#!/usr/bin/env python3
"""Label a random subset of training samples with an LLM-generated exposure score.

The script:
- Ensures an `exposure` column exists on `training_samples`
- Samples unlabeled rows randomly
- Sends title/outcome to an OpenAI-compatible chat-completions endpoint
- Stores the numeric score back in SQLite

Expected model behavior: return a single float in [0, 1].
"""

import argparse
import json
import os
import re
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Optional


SYSTEM_PROMPT = (
    "You are scoring market bet exposition risk. "
    "You must use only the provided title and outcome fields; do not rely on any outside knowledge or assumptions. "
    "Return only one number between 0 and 1, where 0 is low exposition and 1 is high exposition. "
    "Do not return any other text."
)

DEFAULT_DIRECTIVES = (
    "Score exposure based only on title and outcome text. "
    "Never use external context, current events knowledge, or other fields. "
    "Higher score when the bet appears tied to sensitive, potentially information-asymmetric events "
    "(for example regulatory decisions, approvals, central bank decisions, legal rulings, major policy outcomes). "
    "Lower score for generic entertainment/sports/weather style markets."
)

RATE_LIMIT_WAIT_SECS = 30
MAX_RATE_LIMIT_RETRIES = 5


def ensure_table_and_column(conn: sqlite3.Connection) -> bool:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS training_samples (
            title TEXT,
            outcome TEXT,
            exposure REAL
        )
        """
    )

    cols = {
        row[1] for row in conn.execute("PRAGMA table_info(training_samples)").fetchall()
    }
    if "exposure" in cols:
        return False
    conn.execute("ALTER TABLE training_samples ADD COLUMN exposure REAL")
    return True


def fetch_sample(conn: sqlite3.Connection, n: int) -> list[sqlite3.Row]:
    return conn.execute(
        """
        SELECT rowid, title, outcome
        FROM training_samples
        WHERE exposure IS NULL
        ORDER BY RANDOM()
        LIMIT ?
        """,
        (n,),
    ).fetchall()


def parse_score(text: str) -> Optional[float]:
    match = re.search(r"[-+]?\d*\.?\d+", text)
    if not match:
        return None
    try:
        score = float(match.group(0))
    except ValueError:
        return None
    return min(1.0, max(0.0, score))


def llm_score(
    *,
    api_url: str,
    api_key: str,
    model: str,
    title: str,
    outcome: str,
    directives: str,
    timeout_s: float,
) -> float:
    user_prompt = (
        "Directives: To score exposition risk, you should ask yourself how easy would it be for the 'right human' to insider trade that bet\n"
        "Use only title and outcome. Do not use any external knowledge.\n"
        "Do not infer from anything except the two fields below.\n"
        f"{directives}\n\n"
        "Market fields:\n"
        f"title: {title}\n"
        f"outcome: {outcome}\n\n"
        "Return one numeric score in [0, 1]."
    )

    payload = {
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ],
    }

    req = urllib.request.Request(
        api_url,
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="POST",
    )

    for attempt in range(MAX_RATE_LIMIT_RETRIES + 1):
        try:
            with urllib.request.urlopen(req, timeout=timeout_s) as resp:
                body = resp.read().decode("utf-8")
            break
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="ignore")
            is_rate_limited = exc.code == 429 or "too many requests" in detail.lower()

            if is_rate_limited and attempt < MAX_RATE_LIMIT_RETRIES:
                wait_s = RATE_LIMIT_WAIT_SECS
                print(
                    f"Rate limit hit (attempt {attempt + 1}/{MAX_RATE_LIMIT_RETRIES + 1}). "
                    f"Waiting {wait_s}s before retry...",
                    file=sys.stderr,
                )
                time.sleep(wait_s)
                continue

            raise RuntimeError(f"LLM HTTP error {exc.code}: {detail}") from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"LLM request failed: {exc}") from exc
    else:
        raise RuntimeError("LLM request failed after rate-limit retries")

    try:
        data = json.loads(body)
        content = data["choices"][0]["message"]["content"]
    except Exception as exc:
        raise RuntimeError(f"Unexpected LLM response format: {body}") from exc

    score = parse_score(content)
    if score is None:
        raise RuntimeError(f"Could not parse numeric score from model output: {content!r}")
    return score


def update_score(conn: sqlite3.Connection, rowid: int, score: float) -> None:
    conn.execute(
        "UPDATE training_samples SET exposure = ? WHERE rowid = ?",
        (score, rowid),
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Sample training rows and label exposure via LLM"
    )
    parser.add_argument(
        "--db",
        default="./databases/training.db",
        help="Path to SQLite DB file (default: ./databases/training.db)",
    )
    parser.add_argument(
        "--sample-size",
        type=int,
        default=500,
        help="Number of random unlabeled rows to score (default: 500)",
    )
    parser.add_argument(
        "--model",
        default="gpt-4o-mini",
        help="Chat-completions model name",
    )
    parser.add_argument(
        "--api-url",
        default="https://api.openai.com/v1/chat/completions",
        help="OpenAI-compatible chat completions endpoint",
    )
    parser.add_argument(
        "--api-key-env",
        default="OPENAI_API_KEY",
        help="Environment variable containing API key (default: OPENAI_API_KEY)",
    )
    parser.add_argument(
        "--directives-file",
        default="",
        help="Optional text file with custom scoring directives",
    )
    parser.add_argument(
        "--delay-ms",
        type=int,
        default=0,
        help="Delay between requests in milliseconds (default: 0)",
    )
    parser.add_argument(
        "--timeout-secs",
        type=float,
        default=30.0,
        help="Per-request timeout in seconds (default: 30)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be labeled without writing scores",
    )
    args = parser.parse_args()

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    directives = DEFAULT_DIRECTIVES
    if args.directives_file:
        directives = Path(args.directives_file).read_text(encoding="utf-8").strip() or directives

    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row

    try:
        column_added = ensure_table_and_column(conn)
        rows = fetch_sample(conn, args.sample_size)

        print(f"DB: {db_path}")
        print(f"exposure column added: {column_added}")
        print(f"Rows sampled: {len(rows)}")

        if not rows:
            print("No unlabeled rows available.")
            conn.commit()
            return 0

        if args.dry_run:
            for i, row in enumerate(rows[:10], start=1):
                print(
                    f"[{i}] rowid={row['rowid']} | title={row['title']!r} | outcome={row['outcome']!r}"
                )
            print("Dry run complete: no labels written.")
            conn.commit()
            return 0

        api_key = os.getenv(args.api_key_env, "").strip()
        if not api_key:
            print(
                f"Missing API key in env var {args.api_key_env}. "
                "Set it and re-run, or use --dry-run.",
                file=sys.stderr,
            )
            return 1

        labeled = 0
        failed = 0

        for idx, row in enumerate(rows, start=1):
            title = row["title"] if row["title"] is not None else ""
            outcome = row["outcome"] if row["outcome"] is not None else ""

            try:
                score = llm_score(
                    api_url=args.api_url,
                    api_key=api_key,
                    model=args.model,
                    title=title,
                    outcome=outcome,
                    directives=directives,
                    timeout_s=args.timeout_secs,
                )
                update_score(conn, row["rowid"], score)
                labeled += 1
                print(f"[{idx}/{len(rows)}] rowid={row['rowid']} score={score:.4f}")
            except Exception as exc:
                failed += 1
                print(
                    f"[{idx}/{len(rows)}] rowid={row['rowid']} failed: {exc}",
                    file=sys.stderr,
                )

            if args.delay_ms > 0:
                time.sleep(args.delay_ms / 1000.0)

        conn.commit()
        print(f"Completed. labeled={labeled}, failed={failed}")

    finally:
        conn.close()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
