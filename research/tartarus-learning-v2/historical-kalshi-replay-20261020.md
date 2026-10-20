# Exact Kalshi historical trade replay

Both recovered partitions now have immutable, audited SQLite indexes: 244,200 training trades and 48,563 evolution trades across 234 mapped tickers. Original trade objects retain decimal strings, submillisecond timestamps, identifiers and receipt provenance.

`moloch.learning_actions.historical_kalshi` validates original plans, receipt bytes, cursor chains and scoped counts before publishing a completion manifest. Identical duplicate trades are not extra flow; contradictory duplicates abort the build. Public visibility rounds fractional milliseconds upward so a trade cannot appear early.

The training index built in 5.82 seconds and the evolution index in 3.31 seconds. Opening and issuing 5,000 mixed queries took 0.154 seconds and 0.084 seconds respectively. These are CPU provider measurements, not full training throughput. [Actual measurements, counts and hashes](historical-kalshi-replay-20261020.json).

Four focused tests passed for exact fields and causal visibility, declared delivery timing, contradictory duplicates and identical duplicates. The implementation reuses pure receipt validation and performs no network calls.

Historical trades are observations, not executable SIG depth or historical Kalshi books. Mapping availability and original historical delivery remain unverified. The caller must integrate the venue request budget and delayed receipts before training admission. No independent test or production financial state was changed.
