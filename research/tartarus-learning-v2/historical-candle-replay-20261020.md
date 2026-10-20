# Audited SIG candle information replay

The actual local index contains 594,935 candles across 237 exchanges, verified against 1,550 original receipts. Exact OHLCV decimal strings, trade counts, source times and actual retrieval provenance are preserved. Independently decoding the database reproduces the audited normalized source digest.

`moloch.learning_actions.historical_candles` builds the immutable SQLite index and provides bounded chronological queries. A completed minute bar cannot become visible before its close plus the declared delivery schedule. Earlier public information can be queried without admitting future observations. Original retrieval timestamps remain separate from simulated publication times.

The actual build took 22.06 seconds and occupies 59,891,712 bytes. Opening and issuing 10,000 mixed exchange/time queries took 1.106 seconds. These are CPU measurements, not end-to-end training throughput. Under the measured 61-second publication delay, 497,550 candles become visible before the training boundary; the source-start count is 497,561. The difference must not be silently admitted early.

Validation: ten unittest cases passed, comprising five provider cases and five imported source-audit cases. They cover precision, publication causality, partition boundaries, receipt tampering and pinned artifacts. [Actual measurements and hashes](historical-candle-replay-20261020.json).

This is an information provider. It does not reconstruct historical order books, queue priority or executable liquidity. Original historical delivery and competition-start completeness remain unverified. Shared request-budget integration and full historical learner admission are still outstanding. No independent test, GPU training or production financial state was changed.
