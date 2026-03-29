# PMIT Architecture & Design Decisions

> [!NOTE]
> **Status**: This document reflects the current architecture of the operational MVP. Some components (marked as *Planned*) are currently under development.

## Engineering Philosophy

The primary goal was to build a system that is:
1.  **Deterministic in Ingestion**: Polling must never be delayed by downstream analysis.
2.  **Resource Efficient**: Minimizing CPU/Memory overhead during idle periods while remaining highly performant during high-volume trade bursts.
3.  **Extensible**: Allowing for new "Investigator" modules without refactoring the core pipeline.

---

## The Core Pipeline

### 1. The Producer-Consumer Model (MPSC)
We use a **Multi-Producer, Single-Consumer (MPSC)** channel pattern (via `tokio::sync::mpsc`) to decouple the Polling Loop from the Analysis Engine.

-   **Why?** Predicition markets are volatile. A single news event can trigger 10,000 trades in seconds. If the polling loop were synchronous, the delay in scoring these trades would cause the system to "lag" behind real-time.
-   **Backpressure**: The channel is bounded (size 100). If the analysis pipeline falls significantly behind, the system logs a warning rather than crashing or consuming infinite memory.

### 2. High-Speed Persistence (SQLite WAL Mode)
PMIT uses SQLite as a "high-speed local data lake."

-   **WAL Mode**: We enable Write-Ahead Logging (WAL) which allows multiple readers and a single writer to operate concurrently without blocking.
-   **Batch Transactions**: Instead of inserting trades one-by-one, we accumulate raw payloads and perform batch inserts, drastically reducing disk I/O overhead.
-   **Async I/O**: Using `tokio-rusqlite`, database operations are offloaded to a thread pool, ensuring the main async executor is never blocked by sync disk operations.

---

## Cross-Language ML Inference

One of PMIT's unique features is its **Rust coordinator + Python inference engine** architecture.

We implement a **Persistent Worker Process**:
1.  **Initialization**: Rust spawns a single Python child process during startup.
2.  **Readiness**: Rust waits for a `{"ready": true}` signal from the worker's STDOUT before starting the pipeline.
3.  **Communication**: Communication happens via **JSON-over-Pipes** (STDIN/STDOUT).
4.  **Lifecycle**: The Python process is tied to the Rust process; it is killed automatically if the Rust process crashes (via `kill_on_drop`).

### Why not PyO3?
While `PyO3` allows embedding Python directly in Rust, we chose the **Process/IPC** approach for:
-   **Fault Isolation**: A segmentation fault in a Python ML library won't crash the entire Rust monitoring system.
-   **Memory Management**: Python's garbage collector and memory-heavy ML models are restricted to their own process, making it easier to monitor and scale.

---

## Accuracy and Scoring

The **Exposure Scorer** uses `sentence-transformers` (all-MiniLM-L6-v2) to compute the semantic similarity between trade titles/outcomes and known "high-exposure" keywords (e.g., political insiders, regulatory changes).

-   **Softmax Temperature**: We implement a configurable temperature parameter to "sharpen" or "smooth" the exposure scores, allowing for fine-tuning without code changes.
-   **De-duplication**: An LRU cache (size 5000) stores recent transaction hashes to ensure we never process the same trade twice during overlapping polling windows.

---

## 3. Planned: Routing & Profiling (The "Investigator")
The next stage of the pipeline involves deep-dive analysis of "high-exposure" wallets. 

-   **The Profiler**: This component will fetch historical trades for specific wallet addresses to look for patterns of "perfect trades" (e.g., buying minutes before a massive swing).