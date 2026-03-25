# 🔍 PMIT
### An open-source watchdog for prediction market manipulation

> *"Prediction markets claim to aggregate truth. We check if that's actually true."*

---

## What is this?

PMIT is a real-time data pipeline and anomaly detection system for prediction markets (starting with Polymarket). It currently polls the global trades feed, profiles notable trader activity, and is evolving toward statistically robust detection with reproducible public outputs.

The goal is not to profit from manipulation — it's to document it, understand it, and make it visible to the public.

---

## Motivation

Prediction markets are largely unregulated. Proponents claim they efficiently aggregate information. Critics argue they attract manipulation and can influence the very events they predict, particularly in political contexts.

We think the truth is empirical, not philosophical. So we're measuring it.

Specifically, we want to know:
- Do odds move *before* public information breaks?
- Are there accounts with win rates inconsistent with random chance?
- Are there coordinated large bets that precede event resolutions?

If the answer to any of these is yes, average participants deserve to know.

---

## Architecture

```
Polymarket Data API
      │
      ▼
┌─────────────────────┐
│  Rust + Tokio       │  ← async global-trade polling and ingestion
│  Ingestion Pipeline │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  Local SQLite / DB  │  ← raw global trades + account activity data
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  Anomaly Detection  │  ← statistical flagging (ANOVA, clustering, Bayesian)
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  Public Dashboard   │  ← flagged events, suspicious accounts, open dataset
└─────────────────────┘
```

---

## Detection Signals

| Signal                         | Method                                   | Status      |
|--------------------------------|------------------------------------------|-------------|
| Large-trade whale profiling    | Threshold trigger + recent-user cache    | In Progress |
| Odds moving before news        | News feed lag analysis                   | Planned     |
| Large coordinated bets         | Clustering on bet timing + size          | Planned     |
| Anomalous account win rates    | Statistical testing vs. baseline         | Planned     |
| Combined suspicion score       | Weighted composite signal                | Planned     |

---

## Tech Stack

- **Rust + Tokio** — async ingestion pipeline
- **reqwest** — HTTP client for Polymarket Data API
- **SQLite / serde** — local storage and deserialization
- **Python (pandas, scipy)** — statistical analysis and anomaly detection
- **Quarto / matplotlib** — reproducible reports and visualizations

---

## Getting Started

```bash
git clone https://github.com/yourorg/PMIT
cd pmit
cargo build
cargo run
```

Configuration is handled via a `.env` file:

```env
POLYMARKET_DATA_API_URL=https://data-api.polymarket.com
POLL_INTERVAL_SECS=10
GLOBAL_TRADES_LIMIT=1000
LARGE_TRADE_THRESHOLD=1000.0
SQLITE_DB_PATH=./pmit.db
```

---

## Roadmap

- [x] Project scaffolding
- [x] Polymarket Data API integration
- [x] Async global-trade polling
- [x] Local data storage
- [ ] First statistical anomaly detection signal
- [ ] Public dataset release
- [ ] Dashboard / web frontend

---

## Contributing

This is a collaborative research initiative. We're looking for people who care about market integrity and have backgrounds in statistics, systems programming, or investigative journalism.

Open an issue, reach out directly, or just submit a PR.

---

## License

MIT — all data and findings are public and reproducible by design.

---

*Named after Cassandra of Troy — she saw the truth and nobody listened. We're working on the second part.*