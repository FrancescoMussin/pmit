# Structure Refactor Notes

## Goal
Refactor PMIT into a staged real-time pipeline that:
- ingests all trades in real time,
- triages them with an exposure-focused NLP layer,
- enriches high-priority trades with additional context,
- computes a final suspicion signal,
- stores stage outputs for later analysis.

## Core Principles
- Keep raw ingest lossless.
- Keep refactor incremental and reviewable in small independent pieces.
- Favor modularity and single responsibility.
- Keep architecture adaptable to future source/model/storage changes.

## High-Level Flow
1. Ingest
- Collect trades in real time from the selected source and store raw events so no information is lost.

2. Triage
- Perform a fast first-pass assessment (for example on market text/exposure) to prioritize what needs deeper analysis.

3. Route
- Apply policy decisions to decide which trades move forward immediately, which are deferred, and which are ignored for now.

4. Enrich
- Gather additional context (such as maker behavior/history and related market signals) for trades selected for deeper analysis.

5. Score
- Produce a final suspicion signal using enriched context, including confidence and basic reasoning metadata.

6. Sink
- Persist stage outputs for analysis/replay and provide a stable endpoint for future consumers (alerts, dashboards, exports).

## What To Decide During Redesign
- Routing policy (thresholds, sampling, fallback behavior).
- Enrichment depth and failure handling.
- Final scoring strategy and calibration approach.
- Persistence granularity for reproducibility and replay.
- Observability requirements (throughput, latency, queue pressure, error rates).

## Incremental Refactor Approach
1. Create one small independent piece.
2. Review and adjust manually.
3. Integrate only after approval.
4. Repeat until full pipeline is in place.

## Notes
- File-level and struct-level design details intentionally removed.
- This document is now a high-level guide only.
