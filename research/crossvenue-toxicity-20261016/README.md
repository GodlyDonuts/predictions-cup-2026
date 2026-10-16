# Passive shadow-quote toxicity evaluation

This source-only evaluator complements the crossing-return ridge study. It changes no trader, orders, bank, reviews or model approval. It evaluates **conditional hypothetical one-unit passive acquisition**, not maker fills, fill probability, expected trading profit or queue execution.

Before reading labels, a fixed rule classifies each side using only original available Kalshi logit innovations at 1/5/30 seconds. All three must exceed +0.002/+0.005/+0.010 for YES FAVORABLE / NO TOXIC; all must cross the negative thresholds for the reverse. Otherwise both are QUIET. Missing innovation is unavailable, never zero. Original SIG book age must be at most 10 seconds, external availability age at most 30 seconds, and positive SIG spread at most 0.05. Thresholds are explicit research assumptions, not calibrated or tuned using the sample.

At fixed horizons 1/3/10/30 seconds, original YES bid and `1 - original YES ask` are resting BUY prices. Future executable YES bid and `1 - future YES ask` mark hypothetical one-unit liquidation, with actual future depth at least one on both sides. Decimal fee and buffer charges are explicit policy assumptions. There is no deduction that touching prices or public trades filled these quotes. Retaining more favorable conditional markouts is not proof of more profitable execution.

The report shows all matched controls, per-state counts and exact markouts, defensive retained observations, skipped adverse magnitudes **and missed positive magnitudes**, missing-label counts by prospective state, and nonoverlapping market/time clusters. Both sides share the same opportunity set. Clusters cover horizon plus 3-second label slack; they do not establish statistical independence across correlated elections/venues/time. Missing labels may be selective. At least 100 such clusters is a research coverage threshold only: every result still abstains because original maker-fill qualification is unavailable.

```powershell
python moloch/external-data/crossvenue_toxicity.py --input SEALED.jsonl.gz --as-of 2026-10-16T10:22:40.388Z --producer-source ORIGINAL_PRODUCER.py --producer-sha256 EXACT_SHA256 --sample-id-format legacy_unscoped_v1 --output NEW_REPORT.json
python -m unittest discover -s moloch/external-data -p test_crossvenue_toxicity.py
```

The CLI requires an explicitly selected `legacy_unscoped_v1` or `scoped_v2` sample-ID formula and original producer source hash; it never guesses, rewrites IDs or renews timestamps. Shared economics loader bounds original compressed/uncompressed bytes, rows, file count and line sizes; rejects conflicting duplicates, malformed/truncated data and changed files; preserves source bundle SHA. Original decision content hashes and matched cohort hashes freeze the as-of features. Labels must be original prospective counterfactuals with no actual fill, correct exact crossing-return checksum and actual clocks in the declared horizon/slack window. Fees must be known explicit assumptions, never interpreted as verified zero venue fees.

Implementation and synthetic/actual-observer schema tests are separate from live evidence. No sealed real-data result is claimed by this source handoff. This is a defensive hypothesis measurement artifact, not an authorization or production signal.
