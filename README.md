# 🔍 PMIT
### An open-source prediction market anomaly detection system

> [!NOTE]
> **Project Status: Under Active Development (WIP)**
> This tool is currently in its early development phase. The core ingestion and ML scoring pipeline is functional, but deep-dive profiling and visualization modules are still being built.

---

## Motivation & Engineering Goals

PMIT was developed to explore the intersection of **high-throughput asynchronous systems** and **real-time machine learning inference**. Using Polymarket as a live data source, the project implements a robust pipeline capable of digesting thousands of trades per minute while performing semantic analysis to identify potential insider activity.

The core engineering focus was on building a **resilient, decoupled architecture** that maintains ingestion integrity even when downstream analysis (ML scoring) experiences latency.

## Challenge

The challenge is to be able, from only the polymarket APIs, to detect people using their insider knowledge against
people partecipating on the other end of the bet. There are many components to this, since not all markets are
exposed to insider trading (for example the ones about city temperatures) and since insider traders can use different strategies.

> [!IMPORTANT]
> **Strategy Focus**: The current ingestion pipeline is intentionally restricted to **"buy" trades**. This simplified focus allows for more targeted anomaly detection of insider "entry" patterns while avoiding the increased complexity of mapping strategic selling (front-running) in early development phases.


---

## Architecture

PMIT uses a **fully asynchronous, decoupled pipeline** to ensure real-time performance even under heavy market loads. 


1. **Ingest (Fast Path)**
   - The main loop polls the Polymarket Data API and immediately persists raw trades to SQLite using **batch transactions**.
   - Decoupled via **MPSC channels** to ensure polling is never blocked by downstream analysis.

2. **Analysis (ML Scoring)**
   - A dedicated background task manages a persistent **Python worker process** running a **sentence-BERT** model.
   - Communication via JSON-over-pipes avoids the overhead of repeated process spawning.

3. **Routing & Profiling (Planned)**
   - Trades are filtered by an exposure threshold. High-exposure trades will trigger the **User Activity Profiler**, which is currently being architected to fetch maker history snapshots for deep-dive anomaly detection.

---

## Tech Stack

- **Rust + Tokio** — core async coordination
- **tokio-rusqlite** — non-blocking, asynchronous SQLite persistence
- **MPSC Channels** — architectural decoupling of ingestion and analysis
- **Python (sentence-transformers)** — ML-powered exposure scoring
- **reqwest** — async HTTP client
- **SQLite** — high-speed local data lake with WAL mode enabled

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

- [x] **Project scaffolding**
- [x] **Polymarket Data API integration**
- [x] **Async global-trade polling & non-blocking SQLite**
- [x] **MPSC Decoupled pipeline architecture**
- [x] **Python-based exposure scoring engine (sentence-BERT)**
- [x] **Local batch-persistance logic**
- [ ] **Investigator (Deep-dive on specific user patterns)**
- [ ] **Data presentation dashboard**

---

## Contributing

Open an issue, reach out directly, or just submit a PR.

---

## License

MIT — all data and findings are public and reproducible by design.

---