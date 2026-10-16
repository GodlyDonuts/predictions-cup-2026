# Atlas joint election model — source and measured validation

Atlas is a working offline empirical-Bayes predictive model, not a trading permission or a demonstrated source of excess returns. It ingests the actual retained Senate and governor cohorts, fits correlated forecast error, runs held-cycle and earlier-cycle-only diagnostics, then exports a compact integer-only joint-state artifact for the Rust value-surface/semantic compiler.

The first actual fit contains **416 races and 1,291 pollster/race observations**: Senate185/491 and governor231/800, spanning23 election cycles. It uses NumPy2.3.5 from the bundled runtime; SciPy is unnecessary. No live account requests, process changes, provisioning or raw dataset publication occur.

## What is implemented

For each poll the target is actual final Democratic-minus-Republican margin minus the poll margin, in percentage points. A last/newest independent pollster observation is retained at the original study horizon. The Senate study uses45-day forecasts with30days of poll history; governor30-day forecasts with30days of history. Recency weights have a fixed21-day half-life.

The model contains:

- Shrunk office-level mean error and persistent state effects. State-effect error moments omit the same election cycle to avoid erasing its shared shock through an in-sample state fit.
- Pollster house effects estimated from within-race deviations, with empirical variance and partial pooling. A short current publisher name is never guessed to be a historical pollster ID. Unseen houses retain zero mean with the learned pooled house variance.
- A ridge-shrunk within-race time/horizon effect. Current margins and uncertainty use actual field-to-election clocks, not a fabricated publication time.
- Multinomial margin sampling variance from actual reported sample size and shares, plus learned within-race overdispersion. Missing current sample sizes use a declared empirical fallback variance, not an invented sample count.
- Positive-semidefinite national-cycle, region-cycle, state-cycle and race uncertainty. Pair moments compare races from the same election cycle; cycle weighting prevents one large cycle from becoming thousands of independent observations. Component floors and nonnegative variance-budget clipping are explicit assumptions in CONFIG.
- Posterior uncertainty in office/state/house/drift means. Shared factors persist when many polls are combined; only measurement noise decreases with independent evidence.
- Correlated Monte Carlo draws with a shared fixed Student-t4 scale. This is an empirical-Bayes predictive distribution. Variance hyperparameters and the tail family are not claimed to have a fully Bayesian hyperposterior or a proof of correct calibration.

Four regions use the [U.S. Census classification](https://www2.census.gov/geo/pdfs/maps-data/maps/reference/us_regdiv.pdf), not classifications fitted on held-out outcomes.

## Actual validation, without a superiority claim

[validation-summary-20261016.json](validation-summary-20261016.json) retains every held cycle, training years, race counts, probability bins and joint-pair diagnostics.

| Metric | Atlas held-cycle prediction | Same-fold pooled poll-error baseline |
| --- | ---: | ---: |
| Races / cycles |416 /23 |416 /23 |
| Brier, lower better |0.084379880 |0.084485685 |
| Log loss, lower better |0.266093117 |0.263378615 |
| Margin RMSE, points |7.632271 |7.459314 |
| Nominal90% margin interval coverage |87.7404% |87.2596% |
| Correct directions |360 |366 |

The23-cycle block bootstrap's95% percentile interval for paired Brier difference is **[-0.00416476,+0.00474721]**. It spans zero. The very small Brier change does not establish superiority; log loss and margin RMSE worsen. Joint pairwise Brier also does not demonstrate an improvement: the mean per-cycle joint-minus-independent-marginals difference is approximately+0.0001371. These findings must remain alongside the implementation.

The earlier-election-years-only diagnostic covers357 races/20cycles, Brier0.087274266, log loss0.277100304, nominal90% margin interval coverage92.4370%. Its training cutoff is strictly before each tested year. LOCO is retrospective and may train on later years; those two experiments are explicitly different.

Neither diagnostic has original historical release clocks, LV/RV labels, historical executable prices or trade accounting. Eventual-candidate matching and revised archives are selection/vintage limitations. Correlated races are not independent repetitions. Do not relabel the results as untouched as-of forecasts, independent calibrated probabilities, trading PnL or an approved live model.

## Sources and reproduction

The immutable primary polling and governor outcome bytes are locally restricted, not committed:

- [FiveThirtyEight polling archive](https://raw.githubusercontent.com/fivethirtyeight/data/master/pollster-ratings/raw_polls.csv):SHA256 `9f0b185506e59117c150fb988e73b612346effe8e3a8f0d90fdafcad07f54756`.
- [Pinned governor outcomes](https://raw.githubusercontent.com/fivethirtyeight/election-results/d7a7cff101da28f4ff77114450a964874800ca54/election_results_gubernatorial.csv):SHA256 `22ff0589b8ccc4c1c6f6f8c20ea5099d20fa044c696db9392823d59ead1364dc`.
- Senate selection/final margins come from the retained independently checked185-race report and its exact491 selected IDs. `ingest.mjs` verifies the existing summary report hash after Git CRLF-to-LF normalization, then matches original raw poll IDs, party/race identities and outcome margins. The original MIT Senate CSV returnedHTTP403 on this device; this run does not falsely claim to have repeated that original independent identity verification. Both exact checkout-byte and normalized hashes are retained.
- Governor extraction calls the existing identity/outcome cross-check implementation. Poll and governor archives share a publisher; they are not independent certifications of every historical result.

From the repository root, with the two pinned raw CSV files in an absolute private directory:

```text
node research/atlas_model/ingest.mjs ABS_PRIVATE_SOURCE_DIR ABS_PRIVATE_TRAINING_JSON
python research/atlas_model/build_current.py ABS_NEW_PRIVATE_CURRENT_JSON
python research/atlas_model/test_model.py --training ABS_PRIVATE_TRAINING_JSON --current ABS_PRIVATE_CURRENT_JSON
python research/atlas_model/run.py --training ABS_PRIVATE_TRAINING_JSON --current ABS_PRIVATE_CURRENT_JSON --out ABS_NEW_PRIVATE_RUN_DIR
```

`run.py` never overwrites an existing run directory. It writes private0600 files containing the fitted model, full actual validation, exact current inputs' hashes, source-code hash, original source metadata and joint artifact. It uses one NumPy/BLAS thread. The command constructs no transport, account client or credentials. The eleven tests run against the real reconstructed416-race data and cover cycle exclusion, altered held-out labels, actual covariance and sampler agreement, nonvanishing shared uncertainty, house pooling, source-time/duplicate rejection, integer canonical hash/weights, training cutoff and research-only qualification.

The current-input sample uses ten polls from the retained October5 transcriptions: Texas governor and Iowa/Michigan/Ohio/Texas Senate. It retains known source-observation clocks; it does not pretend to have reloaded their publisher originals. Original publisher release times and current sample sizes are missing. Its original decision/expiry clocks are **October5/6**, so the sample is expired and deliberately unsuitable as a current trading signal. Current poll horizons outside training support are explicitly flagged, with no automatic probability promotion.

## Rust artifact contract

[atlas-research-20261016.json](atlas-research-20261016.json) is a16KB actual-fit artifact, not a hand-authored probability fixture.

```text
schemaVersion:1
objective:"ordinary_terminal_race_outcome"
modelSha256: SHA256 of canonical sorted compact UTF8 JSON without modelSha256
trainingCutoffMs, observedAtMs, expiresAtMs: unsigned integer clocks
qualification:{kind:"research", ordinaryOnly:true, ...explicit limitations}
races:[{raceId,state,office}]  // canonical caller-configured race identities
scenarios:[{id:"atlas:<D/R bits>",weightPpm,raceOutcomes:["DEM"|"REP",...]}]
sources:[{sha256,...original provenance}]
calibrationContinuousValueScale:1000000
calibration:{...}
declaredModel:{...fit hash, fixed assumptions, margin moments, sampling seed...}
currentInputSha256, limitations
```

All wire numbers are integers, so Python and Rust canonical JSON do not disagree on floating exponent formatting. Counts/years/clocks remain integers. Continuous calibration values are scaled by1e6, as named by `calibrationContinuousValueScale`; margin means use micropoints, covariance uses micropoints squared. Exact posterior moments for contractual payouts must be derived from the scenario weights in Rust, not substituted from approximate unrounded model moments.

Up to12 races generate at most4096 ordinary outcome vectors. Duplicate Monte Carlo outcomes collapse to deterministic IDs; largest-remainder weights sum exactly1,000,000. Sparse/unseen vectors are not fabricated. Changing inputs, fit/source fingerprints, code, seed or weights changes the artifact hash.

The artifact does **not** say DEM means any particular contract paysYES. An independently reviewed mapping must bind exact tournament/market/exchange/contract IDs, official root hashes, party/candidate predicates, ordinary scenario identities and horizon. `semantic_states` handles the payoff interpretation. RefundPrincipal is cost dependent, not a constant0-or1 payoff. This historical model does not identify void/fusion/replacement/runoff/administrative probabilities; the compiler must not manufacture them or pretend ordinary-only scenarios cover every permitted state. An ordinary posterior cannot authorize a guaranteed package, erase collateral debt, or value forced earlier liquidation. Owner approval, original source verification and runtime financial authority remain separate.
