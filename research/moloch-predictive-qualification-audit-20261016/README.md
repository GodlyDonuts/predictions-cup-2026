# Predictive features are not equivalent-payoff hedges

Read-only audit, October16, source baseline`90fbd456` and its unchanged observer/
analysis modules. No actual sealed-cohort outcomes were opened by this task;
the owner supplied V5 counts23 rows/15 labels and successful exact input replay.
No protocol, labels, source, model, runtime or qualification flag was changed.

**Equivalent resolution and a complete provider-wide trade tape are not universal
prerequisites for a statistical predictor of future SIG executable prices.**
An observed external contract can be a useful covariate even with different
payoffs. That does not make its quoted price the SIG terminal probability, an
executable hedge or a guaranteed price floor.

The concrete code finding is narrower than “the model is blocked by hedge
requirements”: `witness/trace_replay.py:273–276` returns
`eligibleForModelValidation:false` as an **unconditional broad-witness disclaimer**.
It is not a computed conjunction that can identify which requirement failed.
The frozen prospective predictive analyzer actually accepts exact sampler replay
and runs its unchanged fitting/evaluation despite that broad false flag.

## What the frozen protocol really requires

`analysis-protocol.json`, declared14:58:44UTC before trade-mode outcomes, says:

- Population is emitted known-direction observer-visible trade-trigger events,
  not the complete public-trade population; mappings are conditional direct
  party references, `resolutionEquivalenceCertified:false`.
- Targets are future one-share SIG executable-side crossing returns at fixed
  horizons, **not terminal probabilities**. The fixed ten features are three
  original quote logit changes, external/local imbalance, SIG spread, external-
  SIG logit gap, trigger direction/signed log quantity and observed trigger age.
- `optionalSignedFlowWindowsAdded:false`: this model does not require full
  provider signed-flow windows. It needs the individual original trigger and
  actual original quote/lag anchors that supply its declared features.
- It forbids missing-value zero imputation, changed clocks, alternative-event
  rescue, post-outcome tuning and replacing incomplete representatives. All four
  models share the same complete ten-feature/per-horizon cohort.
- It requires fixed chronological purged folds, at least16 training clusters,
  at least100 OOS and100 selected clusters, positive exact after-cost selected
  economics and improvement over the training intercept baseline. All of this
  remains unqualified counterfactual research, without model/order approval.

`analysis/analyze.py:60–109` reruns the known fixed sampler verifier, binds the
same producer/sample/config/mapping/original inputs, and requires
`samplerInputReplayVerified:true`. It does **not** require
`wholeWitnessVerified`, `eligibleForModelValidation`, provider-tape completeness
or equivalent resolution to be true. It then calls the original fixed math.
`analyze.py:415–436` computes coverage/economic abstention independently of those
source-witness flags. `source_innovation.rs:1–4,62–82` likewise explicitly
separates modeled information response from economic payoff equivalence and
allows independently bound source/target roots. None is a calibrated live
artifact merely because its schema or trusted digest validates.

Thus the broad flag is **overstrong/misnamed if read as a universal predictive
research veto**, but there is no proved executable math gate here incorrectly
requiring hedge equivalence. Do not “repair” historical results by flipping it.
V5's23 total rows/15 horizon labels cannot establish the fixed100-OOS/100-selected
requirement even under an unrealistically generous one-cluster-per-row bound.
Actual dedup, complete cases, horizons and folds can only reduce those counts.
Insufficient data remains the concrete statistical barrier independent of the
broad disclaimer. No estimate of OOS profit follows from successful replay.

## Requirements by claim, not one whole-witness boolean

| Evidence dimension | Predicting observed future SIG prices | Hedge/terminal valuation claim |
|---|---|---|
| Exact source bodies, stable identities, original availability and feature computation | Required for the claimed causal features and reproducibility | Also required |
| Exact SIG market/exchange/tournament target and executable-side depth | Required; another context is a different target | Required plus exact contract payoffs |
| Equivalent settlement rules | Not required for a covariate; conditional mapping and semantic differences must be explicit | Required for asserted equivalent hedge; subjective valuation needs separately reviewed basis/exception assumptions |
| Complete provider trade tape | Not required for individual observed triggers or sampled quote changes | Required only when the claim/feature depends on total flow or absence; still not a payoff proof |
| Collector epoch/continuity | Required where a lag/delta/window claims uninterrupted same-source history; otherwise declare observed-input feature and reset/missing policy | Does not itself certify payoffs |
| Complete provider universe / no missing venue updates | Not necessary for a predeclared conditional observed-snapshot target | Necessary for stronger global “first venue update/no event” claims; often unavailable |
| Exact future label rule and actual later availability | Required; no alternative later price or missing-label zero | Needed for empirical execution evidence too |
| Prospective OOS validation and realistic costs/latency/fill economics | Required before empirical strategy claims | Equivalent payout alone does not prove execution profit |

Observational predictive association is not causal intervention proof. “Causal”
here means inputs actually available before the outcome, without lookahead;
shared news, election factors, liquidity and selection can explain correlation.
Incorrect party/year/office mappings may still correlate, but should not be
silently included under a same-race strategy claim. Exact source/target identity
is distinct from asserting equal resolution in every exceptional state.

## Real remaining provenance limits

The replay proves the known sampler consumed specific original recorded inputs,
at original supplied times, and reproduced every emitted decision/label. It does
not independently authenticate the whole provider or infer an earlier missing
input. `crossvenue_observer.py:542–578` normalizes recorded breadth books into
market/exchange/version/clocks/depth; that normalized row does not itself carry
a full authenticated tournament topic. Its `official_sig_realtime` string and
hash chain alone cannot prove which private context produced it.

The genuine producer is capable of a contextual proof:
`native/src/realtime.rs:145–195` constructs the exact tournament topic, validates
the JOIN reply and rejects broadcasts from a different topic or before JOIN.
A prospective evidence packet can bind that actual producer binary/source,
nonsecret original scope/Control hash, successful joined topic/session and
original breadth segment identity, without storing JWTs or auth headers. That
would establish the **prediction target**, not certify a hedge. A filename or a
post-hoc guessed tournament ID is insufficient.

Primary official SIG OpenAPI SHA
`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`,
`/paths/~1realtime~1token/post/description`, explicitly describes best-effort,
one-shot/coalesced delivery, revision gaps, required initial/reconnect/periodic
resynchronization, and a final quiet-topic batch loss that may have no later
gap. Socket health is not complete market coverage. Per-exchange book
`{sequence,at}` is an actual original version, not permission to renew an old
book's clock. A predictor can target its first **consumed eligible** observed
snapshot; it cannot call that the first price state that existed on the venue.

Missing collector epoch proof is feature-specific. A genuine observed trade ID
and one original quote can exist without complete collector lifecycle evidence.
However splicing lag anchors across a restart/unknown source reset is not an
authenticated same-feed innovation. Either prove the declared continuity or
withhold that feature/sample under the frozen protocol. A successor may define
a different observed-cache feature, but must train and deploy that exact
measurement process and disclose informative outages/selection. It may not
rename partial observed counts “total market order flow”.

## Smallest useful prospective successor

Leave V1 protocol/flags/results untouched. Freeze a separately named predictive
qualification protocol before viewing successor outcomes. Keep the actual
recorded source/receipt/availability clocks, exact input replay, original trigger
dedup, fixed feature/target/cost definitions, fixed chronological purges and
coverage minima unless a genuinely new study explicitly predeclares otherwise.
Do not lower sample thresholds after seeing V5 scarcity.

Replace the future ambiguous readiness label with separate dimensions:
`inputReplayVerified`, `targetContextVerified`, `featureEvidenceValid`,
`labelReplayVerified`, `observedPopulationDeclared`, `oosCoveragePass`,
`economicComparisonPass`, `providerTapeComplete`,
`resolutionEquivalenceVerified`, `executionModelApproved`. The last three are
independent claims; the first predictive checks do not imply them. A missing
mandatory feature/target/label dimension withholds that study result, while a
nonrequired provider/hedge dimension remains honestly unavailable.

For the current ten-feature design, collect longer **prospectively** under the
same original input instrumentation, with stable nonsecret topic/collector-run
bindings and explicit reset/censoring. Predeclare event coverage/skips and report
all missing lags/labels. Whole provider tape need not become complete to learn
the conditional target, but sufficient genuine nonoverlapping complete examples
must actually arrive. Do not enlarge old clock bounds, resample old triggers,
retrospectively substitute price-only cohorts, or treat no observed trades as0.

The next go/no-go result should be a frozen-source, same-cohort OOS comparison:
does the external+trigger model beat local and price controls after original
crossing costs on the declared target? A positive result would still need a
separate owned-fill/latency/risk model and exact physical policy approval before
live execution. It would not justify copying external prices into SIG terminal
probabilities or crediting hypothetical crossings as actual profit.

This audit adds no new qualifier or permission path and performs no model fit,
new data acquisition, actual-cohort outcome inspection or activation. It resolves
the logical distinction while preserving the experiment's real data limits.
