# Structure Refactor Plan

## Goal
Refactor PMIT into a staged real-time pipeline that:
- ingests all trades in real time,
- triages them with an exposure-focused NLP layer,
- enriches unusually large or high-priority trades,
- computes a final suspicion score,
- stores all stage outputs (no external alerting/output yet).

## Core Principle
Keep raw ingest lossless. Stage 1 should route and prioritize, not permanently drop events.

## Modularity Requirement (Single Responsibility)
Every major behavior should be behind a small interface and implemented by a struct with one clear purpose.

Design rules:
- One reason to change per struct.
- Depend on traits at stage boundaries, not concrete implementations.
- Keep orchestration separate from infrastructure (HTTP, DB, scraping, model calls).
- Keep domain data models separate from transport DTOs.
- Make data-source swaps (API -> scraper) an adapter change, not a pipeline rewrite.

Practical implication:
- `main` wires dependencies and starts workers.
- Stage workers call trait objects/generics for fetch, triage, enrich, score, and persist.
- To switch Polymarket API to web scraping, implement a new source adapter that satisfies the same trait.

## Proposed Trait Boundaries
- `TradeSource`
  - Purpose: produce a stream/batch of raw trades from any source.
  - Implementations: `PolymarketApiSource`, future `PolymarketScraperSource`.

- `TradeRepository`
  - Purpose: persist and query trade-level records.
  - Implementations: `SqliteTradeRepository`.

- `TriageModel`
  - Purpose: produce fast stage-1 score and reason codes.
  - Implementations: `HeuristicTriageModel`, future `RemoteNlpTriageModel`.

- `Router`
  - Purpose: decide whether to enrich now, later, or by sampling.
  - Implementations: `ThresholdRouter`.

- `EnrichmentProvider`
  - Purpose: fetch maker/context features.
  - Implementations: `PolymarketActivityEnricher`, future `ScrapedContextEnricher`.

- `FinalScorer`
  - Purpose: produce final suspicion score from enriched features.
  - Implementations: `RuleBasedFinalScorer`, future `RemoteModelFinalScorer`.

- `PipelineMetrics`
  - Purpose: report counters/latencies independent of business logic.
  - Implementations: `LogMetrics`, future `PrometheusMetrics`.

## Target Runtime Architecture
1. Ingest stage
- Poll trades continuously.
- Deduplicate by transaction hash.
- Persist raw trade immediately.

2. Triage stage (fast)
- Compute exposure/risk pre-score from title and lightweight features.
- Return score, confidence, and reason codes.

3. Routing stage
- Send high-priority trades to enrichment.
- Also send a random sample of low-priority trades to enrichment to reduce blind spots. (I might change the details here)

4. Enrichment stage
- Fetch maker history and related context with bounded concurrency.
- Persist (store in the database) enrichment artifacts.

5. Final scoring stage
- Compute suspicion score from enriched features.
- Persist score, confidence, reason codes, and model version.

6. Sink stage
- Store outputs only for now.
- No alerting or dashboard action required yet.

## Why Rust Still Makes Sense
- Rust + Tokio remains ideal for high-throughput, low-latency orchestration.
- Whale events are concurrency bursts (fanout, retries, timeouts, bounded workers), which Rust handles well.
- Keep model implementation separate if needed (service call), while Rust controls reliability and backpressure.

## Implementation Sequence (Incremental)

### PR1: Stage Boundaries Without Behavior Change
Objective:
- Keep existing behavior but isolate orchestration and stage responsibilities.

Changes:
- Split current main-loop logic into explicit stage functions.
- Keep current fetch, DB writes, and whale enrichment behavior unchanged.

Acceptance:
- Same ingest throughput.
- Same dedupe behavior.
- Same DB writes for trades and user activity snapshots.

### PR2: Internal Queues and Bounded Concurrency
Objective:
- Decouple stages and avoid ingestion stalls during bursts.

Changes:
- Add bounded tokio mpsc channels between stages.
- Add semaphore limits for enrichment workers.
- Add timeout + retry policy for enrichment HTTP calls.

Acceptance:
- Ingestion remains steady under large-trade bursts.
- Queue depths remain bounded.

### PR3: Replace Print Filter with Structured Triage
Objective:
- Convert boolean title filter into scored triage output.

Changes:
- Replace should_print_trade bool with triage result object.
- Keep initial implementation heuristic-based.
- Add negative-path sampling to enrichment.

Acceptance:
- Every trade gets triage metadata.
- No trade is irreversibly dropped before storage.

### PR4: Persist Stage Artifacts
Objective:
- Make decisions auditable and replayable.

Changes:
- Extend schema for triage output, enrichment metadata, and final scoring output.
- Store model version and timestamps at each stage.

Acceptance:
- End-to-end stage trace exists per trade.

### PR5: Final Scoring Interface
Objective:
- Add final scoring step while keeping output internal.

Changes:
- Define scorer interface contract (input features, output score/confidence/reasons/version).
- Implement initial scorer (rule-based stub or remote model call).
- Persist stage-2 results.

Acceptance:
- Each enriched trade can receive a final stored suspicion score.

## File-Level Refactor Map
- src/main.rs
  - Keep as orchestration entrypoint.
  - Remove business logic from monolithic handle_trade into stage workers.

- src/pipeline/mod.rs
  - Define stage orchestration and worker wiring.

- src/pipeline/types.rs
  - Define shared domain structs passed between stages (`TradeEvent`, `TriageDecision`, `EnrichedTrade`, `SuspicionScore`).

- src/ports/mod.rs
  - Define core traits (`TradeSource`, `TradeRepository`, `TriageModel`, `Router`, `EnrichmentProvider`, `FinalScorer`, `PipelineMetrics`).

- src/adapters/polymarket_source.rs
  - Implement `TradeSource` using Polymarket API.

- src/adapters/sqlite_repository.rs
  - Implement persistence traits against SQLite.

- src/adapters/triage_heuristic.rs
  - Initial stage-1 triage implementation.

- src/adapters/enrichment_polymarket.rs
  - Implement enrichment fetches from Polymarket activity endpoints.

- src/adapters/final_scorer_rule.rs
  - Initial stage-2 scoring implementation.

- src/trade_filter.rs
  - Can be retired or migrated into `adapters/triage_heuristic.rs`.

- src/polymarket/mod.rs
  - Keep API fetchers.
  - Add explicit timeout/retry wrappers for enrichment paths.

- src/db.rs
  - Keep raw trade and user activity snapshot storage.
  - Add stage output persistence for triage and final scoring.

- src/config.rs
  - Add knobs for concurrency, queue capacity, thresholds, sampling, and scorer endpoint.

## Suggested Worker Structs (One Purpose Each)
- `IngestWorker`: pulls from `TradeSource`, dedupes, persists raw trades, emits stage input.
- `TriageWorker`: computes stage-1 decision via `TriageModel`.
- `RouteWorker`: applies routing policy and sampling.
- `EnrichmentWorker`: performs bounded concurrent enrichment via `EnrichmentProvider`.
- `ScoringWorker`: computes stage-2 suspicion score via `FinalScorer`.
- `PersistenceWorker`: writes stage artifacts through `TradeRepository`.

Each worker should be testable in isolation with mocked trait implementations.

## Suggested New Config Variables
- ENRICHMENT_MAX_CONCURRENCY
- ENRICHMENT_REQUEST_TIMEOUT_SECS
- PIPELINE_CHANNEL_CAPACITY
- TRIAGE_THRESHOLD
- TRIAGE_NEGATIVE_SAMPLE_RATE
- SCORER_ENDPOINT

## File Responsibilities (Detailed)

### Existing Files

#### src/main.rs
Purpose:
- Composition root and process lifecycle.

Owns:
- Load config.
- Instantiate concrete adapters.
- Wire worker structs and channels.
- Start and stop the runtime cleanly.

Must not own:
- Trade business logic.
- SQL queries.
- API payload mapping.
- Model scoring rules.

#### src/config.rs
Purpose:
- Typed runtime configuration boundary.

Owns:
- Parse and validate environment variables.
- Hold defaults for pipeline knobs.
- Provide immutable config values to workers.

Must not own:
- Network logic.
- Persistence logic.
- Any triage/scoring decisions.

#### src/polymarket/mod.rs
Purpose:
- Transport adapter for Polymarket HTTP endpoints.

Owns:
- Request construction.
- Response parsing and error mapping.
- Endpoint-specific retry/timeout wrappers.

Must not own:
- Routing decisions.
- Final suspicion scoring.
- DB writes.

#### src/db.rs
Purpose:
- SQLite persistence adapter.

Owns:
- Schema initialization/migrations.
- Insert/query/update for trades and stage artifacts.
- Connection tuning (WAL, busy timeout, sync mode).

Must not own:
- API calls.
- Triage or scoring logic.
- Pipeline orchestration.

#### src/trade_filter.rs
Current purpose:
- Legacy title-based print filter.

Refactor path:
- Migrate logic into `adapters/triage_heuristic.rs` or remove after parity is reached.

Must not own (target state):
- Hard drop logic before raw trade persistence.

### New Files (Planned)

#### src/pipeline/mod.rs
Purpose:
- Stage orchestration and runtime wiring.

Owns:
- Worker lifecycle.
- Channel plumbing and backpressure behavior.
- Graceful shutdown and cancellation.

Must not own:
- HTTP implementation details.
- SQL statements.
- Model internals.

#### src/pipeline/types.rs
Purpose:
- Shared domain payloads between stages.

Owns:
- Stable structs such as `TradeEvent`, `TriageDecision`, `EnrichedTrade`, `SuspicionScore`.
- Stage reason codes and metadata enums.

Must not own:
- API transport structs.
- DB connection logic.
- Worker side effects.

#### src/ports/mod.rs
Purpose:
- Trait interfaces for dependency inversion.

Owns:
- Contracts for `TradeSource`, `TradeRepository`, `TriageModel`, `Router`, `EnrichmentProvider`, `FinalScorer`, `PipelineMetrics`.

Must not own:
- Concrete behavior.
- Runtime wiring.

#### src/adapters/polymarket_source.rs
Purpose:
- `TradeSource` implementation backed by Polymarket API.

Owns:
- Polling and conversion from API response to domain trade events.

Must not own:
- Persistence.
- Routing/scoring logic.

#### src/adapters/sqlite_repository.rs
Purpose:
- `TradeRepository` implementation backed by SQLite.

Owns:
- Persist raw trades and stage outputs.
- Query historical context needed by later stages.

Must not own:
- Network requests.
- Model decision logic.

#### src/adapters/triage_heuristic.rs
Purpose:
- Initial `TriageModel` implementation.

Owns:
- Fast, low-cost triage scoring and reason generation.

Must not own:
- Enrichment fetching.
- Final suspicion scoring.

#### src/adapters/enrichment_polymarket.rs
Purpose:
- `EnrichmentProvider` implementation using maker/activity endpoints.

Owns:
- Bounded concurrent enrichment fetches.
- Mapping enrichment responses into feature payloads.

Must not own:
- Final decision logic.
- Persistence writes.

#### src/adapters/final_scorer_rule.rs
Purpose:
- Initial `FinalScorer` implementation (rule-based baseline).

Owns:
- Stage-2 score, confidence, reason codes, and model version tagging.

Must not own:
- Data collection.
- Queue orchestration.
- DB access.

## Change Isolation Examples
- Replace API with scraper:
  - Add `ScraperTradeSource` that implements `TradeSource`.
  - Keep pipeline workers unchanged.

- Replace heuristic triage with NLP service:
  - Swap `TriageModel` implementation only.
  - Keep routing and enrichment contracts unchanged.

- Replace SQLite with another DB:
  - Swap `TradeRepository` adapter.
  - Keep ingest/triage/enrichment/scoring workers unchanged.

## Operational Metrics to Add Early
- Ingested trades per second.
- Queue depth per stage.
- Stage latency (p50/p95).
- Enrichment success/failure/timeout rates.
- Escalation rate from triage.
- Sampled-negative rate and downstream hit rate.

## Model Strategy Notes
- Two-stage model architecture is recommended now.
- Stage 1 should optimize recall and routing speed.
- Stage 2 should optimize precision with richer features.
- Prevent stage-1 blind spots with random bypass sampling and periodic replay.

## Rough Delivery Estimate
- PR1: 0.5 to 1 day
- PR2: 0.5 day
- PR3: 1 day
- PR4: 1 day
- PR5: 0.5 to 1 day

Estimated total: 3 to 4.5 focused days.
