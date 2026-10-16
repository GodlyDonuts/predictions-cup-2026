# Ordinary inventory lifecycle economics — research only

The [protocol](protocol.json) was frozen at15:42:21UTC before any actual new
outcome analysis: Git-LF SHA256
`04e5dd3ccd678921e34531b8b141cf1e57b33bf340b6162ff990248e1f087235`.
Independent review identified the original reader/fixture's shared-genesis error.
Engineering revision2, declared15:56:27UTC, corrects native `previous:""` versus
dispatch64 zeros. Its new protocol SHA is
`ec397295f2fd5bbc6c6773cd627fec15ffc07418fb0e4c014c58646632dcb5a6`;
the original declaration/hash remain recorded. Economic/model/label/selection
choices are unchanged, and no actual outcome was read between these versions.
No actual cohort has been evaluated by this task. The observer is deterministic
bookkeeping and descriptive estimation, not a fitted policy or execution module.

The original full-fill estimator targets the last completing fill plus its
markout horizon. Native maximum-age planning instead follows the oldest remaining
new lot. This separate observer labels a genuine partial exposure without waiting
for full completion. A first fill at0s and final partial at59s retains the native
60s deadline, rather than moving its mark to119s. If actual FIFO consumption
removes the oldest lot first, the next remaining lot keeps its original clock.

## What is reconstructed

- Original ordinary dispatch intents/attempts/POST ACKs establish attribution;
  native actual ACKs bind physical IDs and actual admitted quantity. Only fills
  executing strictly after the original POST ACK are labelled resting-after-ACK.
  Venue maker role is not inferred. Recovered ACKs cannot create that proof.
- Every actual fill ID is idempotent on exact economic facts. A repeated receipt
  cannot renew first-observed time. Contradictory scope, identity, totals, receipt
  pair, limits or source clocks reject and poison the derived observer.
- Accepted-event FIFO follows the native input sequence, without reordering history
  by later-learned execution times. All captured lots affect position accounting;
  inherited/nonordinary mixed episodes cannot supply ordinary maker labels.
- SELL consumes actual same-side FIFO. An actual opposite BUY closes opposite
  FIFO at one minus its actual buy price, opening only residual shares. ACKs and
  public trades never become fills or redemption income. This reconstructs the
  native accounting convention; it does not independently authenticate venue cash.
- Actual entry/exit fees are allocated proportionally using exact rational
  micro-units. Unknown fees remain unknown. An exact rational is serialized as
  `microNumerator`/`microDenominator`, without rounding fractional fee allocations.
  Recorded native fees are source-attributed, not independently inferred charges.

## Marks remain separate from realized outcomes

There is one first-age mark per exchange/side flat-to-flat episode. The fixed
policy is execution-clock60s, original book freshness10s, horizon slack5s and
native exit top-depth participation50%. The first qualifying original book is
used; shallow, stale, missing coverage or late first evidence cannot be rescued
by choosing a later favorable snapshot. The native trigger is not a guarantee
of physical liquidation at60s.

Known own remaining orders, including unknown attempted liabilities, are removed
from their exact displayed price levels before walking YES bids or complementary
YES asks for NO. Actual terminal evidence cannot erase own liquidity from an
older snapshot: terminal and fill clocks are compared with the book's original
source time. A book predating a known inventory change is unavailable. Current
residual own quantities and future snapshot clocks are never substituted into
past source depth.

A full mark requires depth for the entire currently known exposure and original
explicit complete coverage spanning the oldest execution through book source.
Partial depth/proceeds and the native50%-top-depth slice capacity are diagnostic
only. Future exit fees remain unknown, so `netHypotheticalProfit` is always null.
No mark is added to actual realized totals. Actual closes can precede the age
deadline and then have realized-only status. Late fills invalidate a previously
frozen episode mark rather than revising it silently; gaps/invalidity censor the
existing exposure even if a later complete snapshot recovers.

Complete terminal-zero-fill observations require matching native complete-filled
totals and the original dispatch closed record's order/fill receipt hashes.
Cancellation ACK, native retired, expiry time or order absence is insufficient.
Unknown attempts, partial executions and missing terminal evidence remain distinct
denominator categories. Early terminal-zero observations are not estimates of the
probability of filling a full uninterrupted exposure window.

## Bounded original input and reproduction

```text
python -m unittest discover -s research/moloch-lifecycle-economics-20261016 -p test_estimator.py -v
python research/moloch-lifecycle-economics-20261016/estimator.py \
  --manifest PRIVATE_SEALED_MANIFEST --manifest-sha256 ORIGINAL_SHA \
  --output NEW_PRIVATE_RESULT
```

Manifest schema1 contains `sealed:true`, `protocolSha256`, `asOfUtc` and
`bankManifest`, `dispatch`, `events`, each `{path,bytes,sha256}`. Paths are relative
to the manifest; hashes pin original file bytes, including gzip wrappers. The
original bank manifest must be a sealed actual-account live manifest. Both journals
must begin at sequence1, with native previous empty and dispatch previous64 zeros;
each exact row seal/hash chain is validated
through the existing original-byte parser, whose hash is retained in output.
All three inputs are pinned before replay. No suffix is treated as an empty wallet.
Dispatch rows with an available `at` clock, and original POST request/receipt clocks,
must not exceed the manifest cutoff. Original dispatch `closed` rows have no
timestamp: matching paired native terminal receipt hashes plus the owner's jointly
sealed prefix pins establish membership, not an independently observed dispatch-
close availability time. Complete coverage must match the known exchange/market
and tournament context when bound/available; foreign coverage rejects the observer.

Each input and decoded journal is bounded to2GiB, each journal to1million rows,
each row to16MiB; orders, fills and remaining lots each have100,000-item bounds.
Price parsing permits only the existing0.005 tick's≤1e−12 floating-representation
tolerance, then uses exact integer micro-units. There is no economic price rounding.
Original timestamps are exact integer nanoseconds; JavaScript consumers must not
round them through floating-point JSON numbers. Restart replays the same original
prefixes; there is no loadable state snapshot that can erase unresolved attempts.

Original native row seals/chains are verified by the byte parser; labels retain
that reader-verified native row hash when supplied by the original-journal CLI.
Embedded `sourceRef` fields are only format-checked and reported as native accepted-
chain references: **their original source body/content hash is not independently
recomputed**. `recordedSourceRefSha256` is not a book-body authenticity certificate;
pure programmatic fixtures without a verified journal row report its hash as null.
No official
receipt is fetched, account reconciliation performed or native financial replay
independently executed. The output records these limits. Original settlement or
package-reservation inventory boundaries are unsupported and stop the analysis,
rather than treating inventory disappearance as realized profit. Other native
cash/capital updates do not become trading gains. This reader is not a replacement
for the financial ledger or a claim that all historical branches are implemented.

Thirty-two synthetic checks pass, including an end-to-end sealed/gzip native+
dispatch fixture, partial0/full59/native60, persistent partials, FIFO retargeting,
actual netting/SELL cash, rational fees, inherited/mixed ownership, duplicate/conflict,
restart, terminal receipt-pair proof, unknown/own depth, NO complement, stale/gap/
late/absent coverage and old-book versus later physical state. The original-shaped
native envelope test follows Session::accept's event→sourceRef→sealed-row sequence;
negative tests reject either journal seeded with the other's genesis. These are
serialization fixtures, not an executed Rust Session or native financial replay.
No actual new outcome
files, model fitting, account/network/SSH calls, compiler, live service, production
control or native/runtime source was used or changed. The source is ready for
independent review before any separately authorized sealed-cohort evaluation.

## Later root-run actual original-prefix evaluation

The implementation task above did not inspect a cohort. Subsequently root ran
the frozen reader on separately pinned original sources:281,121 native events,
605 ordinary BUY attempts, zero eligible full-depth lifecycle marks and actual
ordinary realized−188.990000. No fitted model or financial authority follows.
[Actual result, source pins and limitations](actual-evaluation/README.md).
