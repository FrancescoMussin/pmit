# Staleness Diagnosis

## Summary
The staleness is real, and it is **upstream from your app**.

The REST API is serving data that is about **130 seconds old**. This is not a bug in your code; it points to delay or caching on Polymarket's `data-api.polymarket.com` side (for example CDN caching or backend lag).

## Key Findings
- [x] App is working correctly.
- [x] Staleness detection is working correctly.
- [ ] Data source freshness is acceptable.

Current issue:
- The Polymarket REST API appears to be **2+ minutes behind**.

## Why The Website Looks Real-Time
- `polymarket.com` likely uses an internal (non-public) WebSocket.
- Or it may use a different API path not affected by the same caching layer.
- The public `/trades` endpoint being polled appears cached or delayed.

## Options
1. **Accept the delay**: app behavior is correct, but the source has inherent latency.
2. **Scrape `polymarket.com` UI**: possible with headless browser tooling, but fragile and potentially against ToS.
3. **Find another data source**: look for premium/partner APIs with lower latency.
4. **Reverse-engineer internal WebSocket usage**: inspect frontend traffic; possible but complex and brittle.

## Conclusion
Your app, database structure, and polling architecture look solid.

The observed staleness is a **platform/data-source limitation**, not an application correctness issue.

# API limits

On the api documentation, there are limits for each request:

## General

| Endpoint | Limit |
| --- | --- |
| General rate limiting | 15,000 req / 10s |
| Health check (/ok) | 100 req / 10s |

## Gamma API

Base URL: `https://gamma-api.polymarket.com`

| Endpoint | Limit |
| --- | --- |
| General | 4,000 req / 10s |
| /events | 500 req / 10s |
| /markets | 300 req / 10s |
| /markets + /events listing | 900 req / 10s |
| /comments | 200 req / 10s |
| /tags | 200 req / 10s |
| /public-search | 350 req / 10s |

## Data API

Base URL: `https://data-api.polymarket.com`

| Endpoint | Limit |
| --- | --- |
| General | 1,000 req / 10s |
| /trades | 200 req / 10s |
| /positions | 150 req / 10s |
| /closed-positions | 150 req / 10s |
| Health check (/ok) | 100 req / 10s |

## CLOB API

Base URL: `https://clob.polymarket.com`

### General

| Endpoint | Limit |
| --- | --- |
| General | 9,000 req / 10s |
| GET balance allowance | 200 req / 10s |
| UPDATE balance allowance | 50 req / 10s |

### Market Data

| Endpoint | Limit |
| --- | --- |
| /book | 1,500 req / 10s |
| /books | 500 req / 10s |
| /price | 1,500 req / 10s |
| /prices | 500 req / 10s |
| /midpoint | 1,500 req / 10s |
| /midpoints | 500 req / 10s |
| /prices-history | 1,000 req / 10s |
| Market tick size | 200 req / 10s |

### Ledger

| Endpoint | Limit |
| --- | --- |
| /trades, /orders, /notifications, /order | 900 req / 10s |
| /data/orders | 500 req / 10s |
| /data/trades | 500 req / 10s |
| /notifications | 125 req / 10s |

### Authentication

| Endpoint | Limit |
| --- | --- |
| API key endpoints | 100 req / 10s |

### Trading

Trading endpoints have both burst limits (short spikes allowed) and sustained limits (longer-term average).

| Endpoint | Burst Limit | Sustained Limit |
| --- | --- | --- |
| POST /order | 3,500 req / 10s | 36,000 req / 10 min |
| DELETE /order | 3,000 req / 10s | 30,000 req / 10 min |
| POST /orders | 1,000 req / 10s | 15,000 req / 10 min |
| DELETE /orders | 1,000 req / 10s | 15,000 req / 10 min |
| DELETE /cancel-all | 250 req / 10s | 6,000 req / 10 min |
| DELETE /cancel-market-orders | 1,000 req / 10s | 1,500 req / 10 min |

### Other

| Endpoint | Limit |
| --- | --- |
| Relayer /submit | 25 req / 1 min |
| User PNL API | 200 req / 10s |
