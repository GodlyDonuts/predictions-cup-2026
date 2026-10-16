# Independent original-source witness checks

`verify.py` is an offline companion to the frozen analyzer. It verifies original
SIG/collector membership and recorded-book chronology, rather than accepting
embedded hashes as proof of their source. **It never returns whole-witness or
model-validation eligibility.** Existing captures lack an independent complete
observer input/consumption trace. Active trade-v1 data was not opened for this
task; all12 checks use synthetic original-shaped receipts and hash-chain files.

## Frozen input contract

CLI: `python verify.py --manifest SEALED_MANIFEST --manifest-sha256 EXACT_SHA
--output NEW_OUTPUT`. Output is create-new. The manifest has exactly these keys:

```json
{
  "schemaVersion": 1,
  "sealed": true,
  "cohortId": "operator-frozen-cohort-id",
  "asOfUtc": "ORIGINAL_SEAL_CLOCK",
  "sampleFiles": [{"path":"samples.jsonl.gz","bytes":0,"sha256":"ORIGINAL_SHA"}],
  "sigSegments": [{"segmentId":"0","file":{"path":"sig.jsonl","bytes":0,"sha256":"ORIGINAL_SHA"}}],
  "collectorFiles": [{"path":"collector.ndjson.gz","bytes":0,"sha256":"ORIGINAL_SHA"}],
  "observerInputTrace": null
}
```

This example is a structural template, not usable evidence. All descriptors
must pin actual original bytes/hash; `sealed` is the operator's input-freeze
declaration, not independent authentication or financial authority. Source
paths are relative to the manifest, without parent traversal or symlinks. Each
source list has1–32 entries, total compressed/raw input is at most512MiB, and
total decoded input is bounded to512MiB/500,000 rows, with8MiB per
newline-terminated row. Full SIG segments must start at sequence1 with the original empty previous
hash and contain every row through their declared sealed end. A trimmed suffix
cannot be labeled a full segment. Inputs are hash/identity checked before and
after verification. Files are never collected, fetched, rewritten or repaired.

The verifier accepts only `observerInputTrace:null` today. A supplied plain
`complete:true` or timestamp object is rejected, rather than promoted into an
availability proof. The required future trace is described below; this module
does not invent that data or implement an unreviewed trace-authority format.

## What is verified

Original sample/label shape and economic arithmetic reuse the existing frozen
analyzer. The independent source checks then require:

- Each original SIG row's exact byte hash and preceding hash/sequence chain.
  The hash is checked against original numeric bytes, without reserializing
  official floating-point levels. Normalization uses the existing0.005-tick
  tolerance and exact retained level quantities; no rounded price is invented.
- Exact sample market/exchange, original row hash/sequence, source/receipt
  timestamps and derived BBO/depth features against an original book. Matching
  price with different original quantity fails. Conflicting complete depth at
  the same engine version/time fails even if a consumer would ignore a repeat.
- An original public trade body with its retained exact body-text SHA256 and
  JSON. The trigger's trade identity/ticker, execution/receipt clocks, SID/
  sequence, original prices/quantity and direction must match. Direction comes
  only from the documented fields present in that original body, never a price
  touch. Conflicting same-identity economics fail before sample selection.
- Every label's original matching same-scope book body, even when a gap makes
  chronology unavailable. Missing original trade, sample book or label body
  fails; an internally self-consistent sample JSON is insufficient.
- The first original source/receipt-eligible book **in the same recorded
  segment** after the sample's referenced book. Substituting a later, more
  favorable book fails. Late delivery of an old-source book is not mistaken
  for a future-horizon observation. An earlier candidate outside the fixed
  slack cannot be skipped to obtain a label.

Market revision/duplicate handling follows original observer breadth semantics.
Explicit resets, dirty/settlement hints and resync/gap boundaries conservatively
withhold first-recorded chronology across the affected scope. Independent
rotated chains never establish cross-segment continuity by filename or manifest
ordering. This is recorded-input verification, **not** completeness of the
provider's best-effort feed, original account authority or settlement equivalence.

## What remains unavailable

Current SIG breadth records preserve original socket receipt times, but not each
observer consumption time. Raw public trade receipts do not preserve the
collector's processing clock/connection epoch. Exported flow snapshots are
retained inside samples, but the full cache HTTP responses and poll completion
trace are not independently retained. Raw source membership cannot establish
whether the observer saw an earlier eligible book first, or whether a trade was
already visible on an earlier poll that produced no sample.

Accordingly the output separates verified membership/first-recorded-order from
unavailable first-observer-availability/first-consumed-book/processing/epoch/
rotation dimensions. External price/lag-anchor membership is also explicitly
outside this module. Market/exchange IDs in a breadth file alone do not supply
an original private-channel tournament/control binding; that context and
resolution equivalence remain unavailable. Output pins the LF-normalized
verifier/analyzer/normalizer source hashes used for these checks. `wholeWitnessVerified`, `eligibleForModelValidation`,
`providerTapeComplete` and `executionAuthority` remain false. No missing flow,
label or clock becomes zero, fresh or approved. A successful membership check
cannot validate a complete ten-feature model or its held-out economics.

## Minimal prospective original trace needed later

A separate reviewed instrumentation task would need an append-only sealed run
trace, not a caller's retrospective certificate:

1. Run-start original producer/config/mapping/policy hashes, initial source file
   identity/offset/last hash and actual original clock. This must identify the
   real observer start boundary without claiming the unread prefix was consumed.
2. Every actual observer input invocation in one ordered trace: original cache
   route, full raw response bytes/hash, original request/read-completion clocks
   and **exact original `now` argument passed to the sampler**; or exact SIG
   segment/row/hash/offset and original consumption clock. Include unsuccessful
   polls and all inputs that emit no sample. Otherwise first visibility and
   first eligible book remain unproved. Record apply completion separately so
   a crash during an input is not silently treated as successful consumption.
3. Actual reset/epoch/rotation boundaries and source positions. Independently
   chained breadth files need an original trace linking their actual consumed
   boundaries; consecutive filenames are insufficient. Collector processing/
   epoch proof needs original collector evidence rather than a rewritten trade.
4. An immutable terminal seal listing every original trace/source file's raw
   byte count/hash and final trace head, actual stop clock, and continuity gaps.
   Bound storage and stop/seal on capture exhaustion; do not delete evidence and
   then label the remaining trace complete.

Full response storage has a real cost. Hash-only earlier responses cannot prove
that a trade was absent from them; any future reduced representation needs an
independently reviewed membership/nonmembership scheme, not an assumed trust
flag. Replay must use reviewed known sampler semantics and exact input order/
clocks, not execute arbitrary producer code supplied by a manifest. These are
future obligations; no collector/observer instrumentation or source clocks were
changed here.

Validation:12 local tests pass, including genuine-shaped membership, missing/
conflicting original bodies, trimmed chains, changed quantity, first-later-book
substitution, delayed old source, gap censoring, independent rotation, conflicting
same-version depth and counterfeit trace assertions. There were no SSH, account
requests, active-cohort reads, runtime actions or actual model/fill/profit claims.
