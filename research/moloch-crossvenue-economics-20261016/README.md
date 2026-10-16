# First prospective cross-venue economic evaluation

Cutoff2026-10-16T10:22:40.388Z. The sealed source contains200 decision rows
and291 future labels across35 markets.192 decisions have all three requested
external change features. These are prospectively observed SIG/Kalshi data,
not a historical replay made to appear contemporaneous.

[Full original report](first-prospective-report.json),
[compact aggregate summary](first-prospective-summary.json).
The full report SHA256 is
`d373acb0f2830d2506f5c904085cd79d7a72a119b9b00d38c5fe9b9f86b1eb7c`.

| Horizon | Raw future labels | Held-out market/time clusters | Selected trades |
| --- | ---: | ---: | ---: |
|1second|75|36|0|
|3seconds|66|32|0|
|10seconds|81|56|0|
|30seconds|68|29|0|
|120seconds|1|0|0|

All horizons abstain. The fixed seven-feature ridge model is fitted on three
expanding folds with horizon plus availability purging. It estimates separate
YES/NO one-share crossing returns, charges a0.005/share buffer and compares
with an intercept baseline. Zero recorded fee is an explicit assumption for
this counterfactual, not a general fee guarantee. Lower short-horizon prediction
error did not create a selected cost-clearing trade. Insufficient observations
and missing future labels prevent any profitability conclusion.

Cross-market political exposures can be correlated: the market/time clustering
does not prove statistical independence. Hypothetical one-share exits assume
original displayed depth and cannot demonstrate a resting maker fill or full
position liquidation. This evaluation does not authorize production execution.

The retained producer is the original v2 script SHA256
`793eafabe067d4042a27854dd8613c9883b3a679997ac8072a0cb1bceeb983b4`.
Its legacy sample ID omits market/exchange scope. The evaluator requires the
explicit `legacy_unscoped_v1` format and exact producer hash; no source rows
were rewritten to match the newer scoped producer. The original sealed gzip
SHA is`a530b55be1120b80d144d6093e5764a1b60cad134634f3dbce001ac4a4ca6f5aa`.
Private original observations remain on the VPS; these public reports contain
aggregate results and source hashes only.

Implementation: [evaluator](../../moloch/external-data/crossvenue_economics.py),
[observer](../../moloch/external-data/crossvenue_observer.py). Focused checks:
15 economic evaluation and14 observer tests passed. No paper profit or live
alpha is claimed.
