# Offline book-feature baseline

**This milestone is a reproducible software baseline, not a market edge.** The only included dataset and results are **SYNTHETIC-ONLY**. Its signal is deliberately manufactured. No real captured dataset was supplied or found in the repository for this milestone; no real model performance is reported.

The package fits a small regularized linear model to future changes in displayed YES book midprice. It never infers aggressor side, creates fills, computes strategy P&L, reads credentials, imports the scanner, makes market-data requests, submits orders, runs a scheduler, deploys or merges. Node.js 22+ and its standard library are the only runtime dependencies.

## Reproduce from repository root

```sh
node --test ml/test/*.test.mjs
node --test --experimental-test-coverage ml/test/*.test.mjs
node --test bot/*.test.mjs ml/test/*.test.mjs
# Or: cd ml && npm test

# The parent directory must already exist. Each output directory must be NEW.
mkdir -p ml/runs
node ml/cli.mjs develop --input ml/fixtures/synthetic-books.jsonl --config ml/fixtures/config.json --out ml/runs/synthetic-dev-v1
node ml/cli.mjs final --input ml/fixtures/synthetic-books.jsonl --config ml/fixtures/config.json --selection ml/runs/synthetic-dev-v1/selection.json --out ml/runs/synthetic-final-v1

# Optional deterministic fixture regeneration; this refuses to overwrite a file.
node ml/generate-fixture.mjs /tmp/new-synthetic-books.jsonl
cmp /tmp/new-synthetic-books.jsonl ml/fixtures/synthetic-books.jsonl
```

Use a fresh run name if outputs already exist. No `npm install` is necessary. `--help`/no arguments only print help. Unknown commands, `--live`, URLs, duplicate/missing flags and feed-capture input fail. An error writes a structured stderr summary and exit code 1, not a partial success report. New output directories/files use 0700/0600 permissions. Files are exclusive; no overwrite. An I/O failure can leave an incomplete directory: only a valid `complete.json` with matching artifact SHA-256s marks completion. Runtime dataset/run directories are ignored by Git.

Exact fixture seed, command vectors, config/source/runtime hashes and expected results are in [fixtures/run-manifest.json](fixtures/run-manifest.json). `fixtures/expected-synthetic.json` is the deterministic key-result golden file. Each actual report records its own exact CLI argv, Node version, canonical config, seed, source-file SHA-256, individual runtime file hashes and combined implementation hash. Numerical results are IEEE-754 binary64, not monetary accounting; timestamps use exact nanoseconds. Fixed seed alone is not a claim of bitwise equivalence on every future Node/platform version.

## What is learned and measured

Input is an explicit [normalized offline contract](SCHEMA.md), not raw exchange traffic. For each accepted book after the first in a continuity segment:

1. Spread: best ask minus best bid
2. Displayed depth imbalance: `(sumBidQuantity - sumAskQuantity) / (sumBidQuantity + sumAskQuantity)` across all supplied levels
3. Top-level microprice minus midprice: `(askPrice * bidQuantity + bidPrice * askQuantity) / (bidQuantity + askQuantity) - mid`
4. Past midprice change from the previous accepted observation in this segment
5. Past depth-imbalance change from that observation

No IDs, future source data, revised snapshots, future means, full-dataset statistics or labels are features. Depth is the supplied truncated book, not full-market liquidity. No execution size or own-order removal is assumed.

The label is `futureMid - currentMid`, where the future row is the first valid observation with **availability time at least current availability + horizon**, at most `toleranceMs` late, in the **same instrument/session/continuity segment**. There is no nearest-neighbor look-back, interpolation or indefinite carry-forward. The fixture uses 2,000 ms horizon and 500 ms late tolerance. These are fixed software-test parameters, not optimized market horizons. Feature availability is `availableAt`, distinct from callback `receivedAt` and source `sourceAt`.

Past/current books must stay within source-age, arrival-gap and expiry policies. Unknown expiry is rejected. An explicit barrier or bad-quality row blocks the current segment; a later segment ID is needed to resume. Unmarked gaps block rather than silently start a fresh valid run. Versions cannot roll backward across segments/sessions. Equal exact versions never renew freshness. Ambiguous same-instrument availability ties fail closed. These choices are conservative and may discard otherwise usable observations; coverage reports make that loss visible.

## Train, validate, then final test

All instruments share absolute chronological cuts. No shuffled/random split is available. Around each cut, the half-open interval `[cut - embargo, cut + embargo)` is excluded. Embargo must be at least horizon plus tolerance. A train/validation sample's **actual** label time must be strictly before the next cut minus embargo; equality is purged. A validation/test sample's earliest past feature input must be at or after its preceding cut plus embargo. This separates complete feature-to-label information intervals, including look-back data. Across-instrument equal timestamps use the same temporal rules.

Only training samples fit feature means/scales and the target mean. Zero-variance features use scale 1. Five standardized features and a centered target are fit by a pivoted 5-by-5 linear solve for:

`mean(squared error) + alpha * sum(squared coefficients)`

The intercept is not penalized. Positive candidate alphas are declared in the config before the run; the smallest validation MAE wins, with the first/smallest alpha breaking exact ties. Validation is model-selection data, not an unbiased performance estimate. The frozen model is still trained on training only; validation is not silently added to the training set for final scoring.

`develop` reports train/validation scores and test counts/row IDs only, and writes a selection artifact. It emits no test metrics, predictions, features or targets. `final` requires that artifact and reproduces training/validation selection, checking exact source, config, implementation and model hashes before accessing the test score. Editing the dataset, alpha or runtime invalidates the selection. A new explicit final run is required for test output. This is reproducible held-out evaluation, **not a tamper-proof holdout service**: repeated final runs, tuning configuration after seeing test results or unrelated manual exploration can still invalidate the experiment. Future work should pre-register the experiment and reserve genuinely unseen captures.

Metrics are sample-weighted MAE (primary), RMSE and target mean in YES price-change units, compared with both **zero change** and the **training-target mean**. Brier score is inapplicable to this regression target. A seeded whole-session cluster bootstrap reports a descriptive 95% interval for ridge MAE minus zero-change MAE only with at least five session clusters. It preserves within-session row/cross-market dependence, but assumes sessions represent independent blocks; that is not established by an ID. It is not a significance, effective-sample-size or profitability claim. Overlapping forward labels and shared political regimes remain important dependence limitations.

## Insufficient real data is an error

All runs must meet declared minimum sample/session counts for each split. `dataKind: real` additionally enforces non-lowerable floors of 200/100/100 train/validation/test samples, at least five sessions in **each** partition, and at least a one-hour span between earliest and latest retained sample availability times in each partition. Toy input relabeled real fails. These deliberately modest rejection floors are **not enough to prove representativeness or authenticity**. The producer's provenance, session definitions, normalization and market coverage require separate audit. Fake session IDs or a false `dataKind` cannot be detected cryptographically by this package.

The fixture has 384 books, 16 synthetic sessions, 336 bounded labels (87.5% of source books), 16 warmup rows, 32 unmatched tail labels and six split exclusions. Retained train/validation/test counts are 126/102/102. Exact synthetic metrics are committed solely as golden correctness outputs. The artificially predictive fixture must not be used to advertise a trained edge, returns or fill calibration.

## Integration boundaries and next evidence

This PR is based on main and modifies **only `ml/`**. It has no import dependency on unmerged paper PR #5 or feed PR #6. The reviewed [feed schema at f21c1e8](https://github.com/GodlyDonuts/predictions-cup-2026/blob/f21c1e8556a6c13fc024521cb089cdee9527124e/feed/SCHEMA.md) informs the contract. It explicitly distinguishes `sig-feed-capture` from its synthetic replay format; neither can be passed into this CLI. A capture reconstruction/normalization adapter is **not implemented or claimed**. PR #5's availability-time split semantics informed this design; its code is not silently copied/imported.

Before real use, separately implement and review an adapter that reconstructs accepted recovery and invalidation lifecycle, preserves source/receive/processing times, scoped identities, book versions and capture hashes, rejects unknown-expiry/gap/recovery/settlement intervals and emits this schema with audited session/segment boundaries. Never assume a captured snapshot alone means accepted recovery. Current feed REST expiry metadata is unknown and therefore insufficient under this ML policy without later valid evidence. Official trades lack aggressor side and own-order attribution; there is no conversion into paper fills.

Then collect authorized representative read-only observations, fix the experimental protocol before final evaluation, audit time/coverage/session dependence and compare held-out prediction errors. Fill/latency/fee/impact calibration, settlement handling, realistic strategy risk and all execution authorization remain separate future work. Nothing here activates a bot or changes execution ownership.
