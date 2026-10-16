# Prospective, original-input-admitted trade analysis

This implements the unchanged analysis protocol declared at
**2026-10-16T14:58:44Z**, normalized protocol SHA256
`dbf827a8cab904595b6173873949cd8c5f9666769754993b5c7636344bddc291`.
Development uses synthetic fixtures only; no actual cohort outcomes were read.

The parent-directory reader from `597fd04a` already implements the frozen
source checks, global economic identity dedup, representative selection,
time-window clustering, common ten-feature cohorts, fixed calendar/purges,
training-only scaling, ridge1 and four model comparisons. This adapter compiles
that exact checked repository source (LF hash `6fb96218a54e1d6aa2b46d524cda1db1afab35f5f06c5bdaf8c16b324ad484e2`)
and the checked original-input verifier (LF hash
`38e9b96cbc7030ba5c47ccd5e8ffb59192124e678a20b7132a7d05415b212122`).
Code hashes permit repository LF/CRLF spelling; original **producer** hashes
remain exact raw-byte hashes and are never newline-normalized. No manifest
selects executable code and cached `.pyc` files are not trusted.

## Input and command

```
python analyze.py --manifest ABSOLUTE_FROZEN_MANIFEST \
  --manifest-sha256 ORIGINAL_MANIFEST_SHA256 --output NEW_REPORT_JSON
python test_analyze.py -q
```

Use the original parent reader's sealed schema1 manifest: `asOfUtc`,
`analysisProtocolSha256`, and prospectively named `cohorts` containing producer,
sampling policy, configuration and mapping hashes plus sample file pins. Each
cohort now additionally requires:

```json
{
  "inputReplay": {
    "manifest": {"path": "original-input-manifest.json", "bytes": 123, "sha256": "..."},
    "result": {"path": "exact-original-replay-result.json", "bytes": 456, "sha256": "..."}
  }
}
```

These are original pre-pinned artifacts, not booleans asserting verification.
The tool reruns the fixed verifier against the retained original cache/trace/SIG
bytes and requires exact canonical equality with the separately pinned replay
result. The original replay must be trade mode, and its original sample member,
raw producer, configuration and mapping hashes must match **this same cohort**.
Only one full original trace sample file per cohort is accepted; fragments or
borrowed verification cannot create a different population. The original trace
seal must precede the frozen analysis cutoff. Missing evidence, changed code,
changed pins, malformed provenance and unresolved verification fail explicitly;
no subset or alternative producer is silently substituted.

An optional cohort `status` file pin can retain the original stopped trade
status/counters (`state:stopped`, original policy SHA and `triggerDisabled:false`).
It is labeled externally pinned, not seal-attested. If absent, producer skip
counters are explicitly unavailable; absent skipped identities are never
reconstructed or assigned zero outcomes/features. Hashes and identities are
checked again after analysis. Local and original private paths remain private
operator inputs, not material to publish as public source evidence.

## Fixed economics and chronology

Global ticker/trade identity conflicts quarantine the entire event. Exact
Decimal equality distinguishes economic changes from equivalent quantity/price
spellings. Earliest representatives are chosen before features/labels; missing
ones cannot be rescued by a later alias. Per-horizon windows consume their full
horizon+3s interval before completeness filtering. Every model uses the same
finite ten-feature and both-side-label cohort at that horizon. Calendar quarter
boundaries use all canonical representative times before feature/label filters.
Training requires decision+horizon+3s strictly before test start and actual
original label availability no later than that start.

The ten fixed columns remain the seven declared price/local features plus
original taker direction, signed log1p original quantity and original
execution-to-observer age. No interactions, flow-window additions, clipping,
zero imputation, inferred direction, changed ages or threshold/horizon search.
Models are unchanged BBO, SIG-local ridge, price-seven ridge and price-plus-
trigger ridge. Both sides use exact displayed one-share executable capacity,
original crossing returns, assumed fee0 and buffer0.005. These are declared
counterfactual costs, never a venue fee assertion or actual fill accounting.

All models abstain together on a fold with fewer than16 training clusters or a
numerical fit failure. OOS requires100 clusters and100 selected clusters,
positive exact net sum and improvement over the fold-training intercept on
both paired MSE and selected economics. YES wins an exact predicted tie; only
strictly positive predictions select an action. Every horizon/comparator,
count/hash/variation/missingness and failure reason remains reported, with no
post-outcome winning-model selection.

A decimal mean may repeat. The adapter labels its displayed decimal expansion
as such and additionally supplies its exact sum/count rational representation,
without changing sums or decisions. Zero-OOS economics are explicitly
`unavailable_no_oos`; an empty sum is not presented as observed profitability.

## Interpretation and checks

Successful input replay proves these recorded callbacks/first-consumed choices,
not a complete provider oracle. Unread source observations and independent
rotations remain disclosed; no synthetic continuity or new post-outcome filter
is added. Provider trade-tape completeness, collector processing/epoch,
tournament topic and settlement equivalence are unresolved. Whole witness,
model approval, actual profit and financial authority remain **false**, even
if descriptive economic thresholds eventually pass. A new calibrated/runtime
model needs separate held-out evidence and authorization. This study does not
export a `source_innovation::Model`, terminal probability or maker-fill model.

Fourteen local synthetic checks pass: genuine actual sampler/input replay
binding and insufficiency; borrowed/forged result or sample refusal; original
config/mapping/cutoff/producer checks; exact side costs/fractional depth; alias
and spacing before completeness; equivalent-decimal versus real economic
conflicts; common OOS cohorts; strict purge/availability and training-only
scaling; all-model numerical abstention; strict positive threshold/YES tie and
both coverage minima; fixed-code/protocol refusal. Parent independently
reproduces before any actual dataset analysis or publication. No network,
account client, production, frozen protocol or existing source changes.
