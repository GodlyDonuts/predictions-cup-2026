# Passive shadow-quote toxicity evaluation

This evaluator complements the crossing-return ridge study. It changes no trader, orders, bank, reviews or model approval. It evaluates **conditional hypothetical one-unit passive acquisition**, not maker fills, fill probability, expected trading profit or queue execution.

The parent ran the first real sealed study at cutoff2026-10-16T10:22:40.388Z:
200 decisions/291 future labels. The1/3/10/30-second matched market/time
cluster counts were70/64/74/59; **all matched states were QUIET**, with no
TOXIC or FAVORABLE cases. This does not qualify a cancellation rule. Data were
recorded prospectively; these fixed rules were specified afterward, before this
report, and are not a policy frozen before that capture. A later independent
capture is needed. [Original report](first-prospective-report.json), SHA256
`4e980319907ed3abd4e1f7c7e4aae27602188f97fcda99d1b53461dbf5701b11`.
The report retains its original evaluator hash`81e2afc1…`; later clock-validation
improvements do not rewrite that evidence. Its0.005/share buffer is an explicit
counterfactual charge, distinct from recorded fees or actual realized loss.

Before reading labels, a fixed rule classifies each side using only original available Kalshi logit innovations at 1/5/30 seconds. All three must exceed +0.002/+0.005/+0.010 for YES FAVORABLE / NO TOXIC; all must cross the negative thresholds for the reverse. Otherwise both are QUIET. Missing innovation is unavailable, never zero. Original SIG book age must be at most 10 seconds, external availability age at most 30 seconds, and positive SIG spread at most 0.05. Thresholds are explicit research assumptions, not calibrated or tuned using the sample.

At fixed horizons 1/3/10/30 seconds, original YES bid and `1 - original YES ask` are resting BUY prices. Future executable YES bid and `1 - future YES ask` mark hypothetical one-unit liquidation, with actual future depth at least one on both sides. Decimal fee and buffer charges are explicit policy assumptions. There is no deduction that touching prices or public trades filled these quotes. Retaining more favorable conditional markouts is not proof of more profitable execution.

The report shows all matched controls, per-state counts and exact markouts, defensive retained observations, skipped adverse magnitudes **and missed positive magnitudes**, missing-label counts by prospective state, and nonoverlapping market/time clusters. Both sides share the same opportunity set. Clusters cover horizon plus 3-second label slack; they do not establish statistical independence across correlated elections/venues/time. Missing labels may be selective. At least 100 such clusters is a research coverage threshold only: every result still abstains because original maker-fill qualification is unavailable.

```powershell
python moloch/external-data/crossvenue_toxicity.py --input SEALED.jsonl.gz --as-of 2026-10-16T10:22:40.388Z --producer-source ORIGINAL_PRODUCER.py --producer-sha256 EXACT_SHA256 --sample-id-format legacy_unscoped_v1 --output NEW_REPORT.json
python -m unittest discover -s moloch/external-data -p test_crossvenue_toxicity.py
```

The CLI requires an explicitly selected `legacy_unscoped_v1` or `scoped_v2` sample-ID formula and original producer source hash; it never guesses, rewrites IDs or renews timestamps. Shared economics loader bounds original compressed/uncompressed bytes, rows, file count and line sizes; rejects conflicting duplicates, malformed/truncated data and changed files; preserves source bundle SHA. Original decision content hashes and matched cohort hashes freeze the as-of features. Labels must be original prospective counterfactuals with no actual fill, correct exact crossing-return checksum and actual clocks in the declared horizon/slack window. Fees must be known explicit assumptions, never interpreted as verified zero venue fees.

Original external receipt age is independently capped at 20 seconds, inclusive, matching the observer. Late `availableMs` never renews the original `receivedMs`; both remain causal. Older producers retain only those two clocks. Original WS `sourceTsMs` (legacy `sourceTs` also supported) may lead original receipt by at most 1,000 ms, but must be no later than original first availability and decision, independently no older than 20 seconds. A server clock 14 ms ahead of receipt can therefore be used once first availability is after that source clock. No timestamps are clamped or renewed; conflicting named source clocks, source after availability, excessive skew, missing or negative receipt clocks fail closed. Reports include the actual evaluator source SHA-256: new evaluator results do not rewrite hashes/results of older sealed studies.

Implementation and synthetic/actual-observer schema tests are separate from the
parent's sealed real-data result above. Current focused tests:17 passed. This
is a defensive hypothesis measurement artifact, not an authorization or
production signal.
