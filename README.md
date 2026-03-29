# 🔍 PMIT
### An open source prediction market anomaly detection system

---

## What is this?

PMIT is a real-time data pipeline and anomaly detection system for prediction markets (starting with Polymarket). It currently polls the global trades feed, looks at markets exposed to insider traders, and tries to spot suspicious actors

It's just a fun project that i used to learn a couple of things
---

## Challenge

The challenge is to be able, from only the polymarket apis, to detect people using their insider knowledge against
people partecipating on the other end of the bet. There are many components to this, since not all markets are
exposed to insider trading (for example the one about city temperatures) and since insider traders can use different strategies.


---

## Architecture

Here's a rough outline of how this is all intended to work. Keep in mind that this is subject to change.

1. Ingest
- Collect trades in real time from the selected source and store raw events so no information is lost.

2. Exposure
- Perform a fast first-pass assessment (for example on market text/exposure) to prioritize what needs deeper analysis.

3. Route
- Apply policy decisions to decide which trades move forward immediately, which are deferred, and which are ignored for now.

4. Context
- Gather additional context (such as maker behavior/history and related market signals) for trades selected for deeper analysis.

5. Investigate
- Perform an additional analysis on the additional data to decide if the selected trades are suspicious or not.


---

## Tech Stack

- **Rust + Tokio** — async ingestion pipeline
- **reqwest** — HTTP client for Polymarket Data API
- **SQLite / serde** — local storage and deserialization
- **Python (pandas, scipy)** — statistical analysis and anomaly detection

---

## Getting Started


Clone the repository and build the Rust backend:

```bash
git clone https://github.com/yourorg/PMIT
cd pmit
cargo build
```

Set up the Python environment (for exposure scoring and analysis):

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

Then run the pipeline:

```bash
cargo run
```

Configuration is handled via a `.env` file:

```env
POLYMARKET_DATA_API_URL=https://data-api.polymarket.com
POLL_INTERVAL_SECS=60
GLOBAL_TRADES_LIMIT=10000
LARGE_TRADE_THRESHOLD=1000.0
EXPOSURE_THRESHOLD=0.6
EXPOSURE_TEMPERATURE=0.05
TRADES_DB_PATH=./databases/trades.db
USER_HISTORY_DB_PATH=./databases/user_history.db
TRAINING_DB_PATH=./databases/training.db
```

---

## Roadmap

- [x] Project scaffolding
- [x] Polymarket Data API integration
- [x] Async global-trade polling
- [x] Local data storage
- [x] Modular pipeline refactor (staged ingest, exposure, routing, context, investigation)
- [ ] Python-based exposure scoring engine (deeper dive)
- [ ] Investigator
- [ ] Data presentation

---

## Contributing

Open an issue, reach out directly, or just submit a PR.

---

## License

MIT — all data and findings are public and reproducible by design.

---