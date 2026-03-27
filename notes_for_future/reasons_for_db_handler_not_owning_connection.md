# Database Handler Connection Strategy (Conversation Notes)

## Conversation

**Me:** Shouldn't the database handler keep the connection struct?

**Reasonable answer:** Yes, that is a valid design. Keeping a persistent connection inside the handler can be cleaner.

**Why we did not force that immediately:**
- The app performs writes from normal flow and from spawned async tasks.
- A single shared `rusqlite::Connection` across concurrent tasks requires synchronization.
- If done naively, this can create contention or unsafe usage patterns.

**Options we considered:**

1. Open per operation (current approach)
- Pros: simple, robust, easy to reason about.
- Cons: small overhead per write.
- Best when: still refactoring and write rate is moderate.

2. One shared connection in `DatabaseHandler`
- Pros: less open/close overhead.
- Cons: needs locking/serialization; can bottleneck under bursts.
- Best when: write patterns are predictable and low concurrency.

3. Connection pool (long-term scalable option)
- Pros: better concurrency and throughput.
- Cons: more moving parts and setup complexity.
- Best when: sustained throughput grows and DB latency matters.

**Me:** In the end I insert one batch at a time, so opening the connection for each batch should be fine, right?

**Answer:** Yes. For batch-oriented writes at non-extreme frequency, this is a good tradeoff.

## Current Decision

For now we keep:
- Open connection
- Write batch
- Close connection

This keeps the system simple and avoids premature concurrency complexity.

## Revisit Triggers

Re-evaluate this decision if one or more of these appear:
- DB write latency becomes a noticeable part of total poll-cycle time.
- Queue/backlog grows during ingest bursts due to DB pressure.
- Profiling shows connection open/close overhead is material.

At that point, move directly to a small connection pool instead of a single globally shared connection.