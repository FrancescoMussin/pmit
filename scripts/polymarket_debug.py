#!/usr/bin/env python3
import time
from typing import Any, Dict, List, Set

import requests

DATA_API_URL = "https://data-api.polymarket.com"
LIMIT = 100
POLL_SECONDS = 2
LARGE_TRADE_THRESHOLD = 1000.0  # size * price


# Optional simple filter, similar spirit to your TradeFilter usage.
def should_print_trade(trade: Dict[str, Any]) -> bool:
    # For debugging stale feeds, print all trades by default.
    return True


def fmt(value: Any, default: str = "N/A") -> Any:
    return value if value is not None else default


def fetch_global_trades(session: requests.Session, limit: int) -> List[Dict[str, Any]]:
    ts = int(time.time() * 1000)
    url = f"{DATA_API_URL}/trades?limit={limit}&_t={ts}"
    response = session.get(url, timeout=10)
    response.raise_for_status()
    data = response.json()
    if not isinstance(data, list):
        return []
    return data


def handle_trade(trade: Dict[str, Any]) -> None:
    maker_address = trade.get("proxyWallet") or trade.get("maker_address") or "UNKNOWN"
    transaction_hash = trade.get("transactionHash") or trade.get("transaction_hash") or ""
    side = fmt(trade.get("side"), "N/A")
    asset = fmt(trade.get("asset"), "N/A")
    title = fmt(trade.get("title"), "Unknown Market")
    outcome = fmt(trade.get("outcome"), "N/A")

    size = float(trade.get("size") or 0.0)
    price = float(trade.get("price") or 0.0)
    total_value = size * price
    timestamp = int(trade.get("timestamp") or 0)

    if should_print_trade(trade):
        print(
            f"TRADE: {title} [{outcome}] | side={side} asset={asset} "
            f"| size={size:.2f} @ ${price:.4f} (value=${total_value:.2f}) "
            f"| maker={maker_address} | tx={transaction_hash[:10]}... | ts={timestamp}"
        )

    if total_value >= LARGE_TRADE_THRESHOLD:
        print(f"  WHALE ALERT -> maker={maker_address} value=${total_value:.2f}")


def main() -> None:
    session = requests.Session()
    seen_hashes: Set[str] = set()
    stale_streak = 0
    stale_warn_secs = 20

    print(f"Polling {DATA_API_URL}/trades every {POLL_SECONDS}s (limit={LIMIT})")
    while True:
        try:
            trades = fetch_global_trades(session, LIMIT)

            newest_ts = max((int(t.get("timestamp") or 0) for t in trades), default=0)
            now_ts = int(time.time())
            lag = now_ts - newest_ts if newest_ts else 0

            if newest_ts and lag >= stale_warn_secs:
                stale_streak += 1
                print(f"[STALE FEED] newest trade is {lag}s old (streak={stale_streak})")
            else:
                if stale_streak > 0:
                    print(f"Feed recovered after {stale_streak} stale poll(s). lag={lag}s")
                stale_streak = 0

            new_count = 0
            # mimic your chronological print style
            for trade in reversed(trades):
                tx = trade.get("transactionHash") or trade.get("transaction_hash")
                if not tx:
                    continue
                if tx in seen_hashes:
                    continue
                seen_hashes.add(tx)
                new_count += 1
                handle_trade(trade)

            if new_count == 0:
                print(f"(No new trades in the last {POLL_SECONDS} seconds)")

        except Exception as error:  # broad by design for quick debug script
            print(f"Error fetching trades: {error}")

        time.sleep(POLL_SECONDS)


if __name__ == "__main__":
    main()
