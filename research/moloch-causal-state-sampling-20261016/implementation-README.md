# Offline coverage derivation

`audit_coverage.py` implements the frozen [protocol](protocol.json), raw LF SHA256
`153f93fa21ac935d3aea2aa65c6733326f1d47e678594e30d4b63cd2fd2dce8a`.
The original README/protocol, observer and verifier are untouched. This is a
finite offline command, not another collector or background process. It has no
HTTP call, account client, fit, trading capability or experiment activation.
Only synthetic inputs have been tested at source handoff.

Run **after** the original V5 normal seal, through the existing offline operator
workflow. The program refuses to begin before **18:17:03 UTC, October 16**;
it also independently requires the original normal seal. Do not inspect the
actual V5 sample/label values to select this audit or change its bounds.

```text
python -B audit_coverage.py --manifest ABSOLUTE_AUDIT_MANIFEST.json \
  --manifest-sha256 EXACT_AUDIT_MANIFEST_SHA256 --output NEW_OUTPUT.json
```

Output is create-new; no input, source or active journal is written. File hashes,
lengths and identities are checked before/after. Paths below are examples, not
source evidence. The small audit manifest has exactly these fields:

```json
{
  "schemaVersion": 1,
  "protocol": {"path":"protocol.json","sha256":"EXACT_FROZEN_LF_SHA","bytes":12723},
  "inputReplay": {
    "manifest": {"path":"original-v5-replay-manifest.json","sha256":"ORIGINAL_SHA","bytes":0},
    "result": {"path":"original-v5-replay-result.json","sha256":"ORIGINAL_SHA","bytes":0}
  }
}
```

Use the original complete input-replay manifest defined by
[`trace_replay.py`](../moloch-trade-trigger-20261016/witness/trace_replay.py).
It pins seal, compressed trace, sealed samples, producer, mapping and every
original SIG segment. Its source paths are absolute; audit-level paths may be
relative to this audit manifest. Obtain the protocol's exact Git LF blob, not
a differently encoded Windows working copy. There is no caller `verified:true`
or adjustable clock/window/model-probability admission API.

The audit loads only the fixed repository verifier with reviewed LF hash
`a51f89856d4848d4bc576d8aa15e0e51db942513db21fd8dac8a09d53702d57b`.
It reruns exact-input verification and requires byte-canonical result equality
to the retained proof, trade mode and the exact V5 producer hash
`2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529`.
A stored positive flag cannot replace replay. The original verifier necessarily
parses original emitted economic fields to check equality; **the coverage pass
does not read those output rows or evaluate their values**. No price, quantity,
return or PnL is emitted by the audit.

## Causal pass and denominator

After verification, one bounded sequential pass begins at the original observer
`run_start`, not a 17:55 cache reset. Original cache bodies, callback order,
`samplerNowMs`, apply completion, original SIG byte membership and rotation
boundaries come from the verified trace. The reviewed sampler's validators
normalize original external flow and SIG books. Its sampling function is
replaced only inside this isolated audit object; it creates no old-style
decision samples or economic pending labels. Invalid/partial flow is a path
diagnostic, never a fabricated unique trade denominator.

First-seen trade identities are consumed throughout warmup, even if excluded.
Enrollment is the preregistered half-open **[17:55,18:15)** interval, capped at
5,000 witnessed exchange units. Each unit is counted before SIG freshness,
one-second label spacing or pending-capacity filtering. Metadata availability
ends at the earliest of the normal seal, 18:17:03, or 123 seconds after reaching
the enrollment cap. Missing horizons are timed out; an early seal right-censors
unfinished windows. Exact boundary instants are included in availability, and
a late callback cannot backdate a horizon.

The published original mapping schema normally binds only SIG **market** IDs.
The program inspects its pinned `markets[].sigMappings[]`: a literal, nonempty
string `sigMarketId`/`sigExchangeId` pair is an explicit source-bound reference
if present. Otherwise a previously validated genuine SIG book callback must
have witnessed that exchange. No single-exchange assumption, future backfill,
title inference or arbitrary manifest-supplied exchange map is used.
`valid_market_trigger_observations` and `target_exchange_witness_missing` are
therefore separate from `witnessedExchangeUnitDenominator`. The latter is always
reported **not known complete**. Missing/gapped/invalid state with an existing
binding stays in that denominator. A target with no exact binding abstains.

Each unit retains hashes and original trade/source/receipt/collector-availability
and observer-decision clocks, original SIG book-version/source/receipt/availability
clocks, state age bands, gap flags and optional original request clock. Stale
depth is never marked executable. Equal engine versions do not renew first
availability, including across a gap/resync. Only the greatest version and its
depth digest/earliest clocks are retained per scope, not an unbounded version
history. Conflicting equal versions provide no numeric state. A cross-gap
rollback makes ordering unknown for that scope for the remainder of this audit;
no original epoch witness exists to repair it. The output contains disjoint first-stage exclusions, separate
poll/duplicate diagnostics, age-band counts and horizon availability by age,
scope and minute. Those diagnostic counters are not added to the denominator.

Availability requires an original fresh target book with source, receipt **and
observer availability** inside the frozen horizon/slack window. Gaps censor
cross-gap horizons; a secondary next-observation endpoint reports delay and its
gap flag only. Initial missing state can still have a later availability witness.
Absent observations are never zero price changes. Provider tape completeness,
pre-run SIG history, cross-segment continuity, resolution equivalence and model
or execution qualification remain unverified/false. Source hashes establish
integrity and reproducibility, not authentication or trading authority.

## Checks

Source handoff: **29/29 synthetic checks passed in 4.947 seconds** on the bundled
Python; AST parsing, CLI help and frozen README/protocol byte checks also passed.

```text
python -B -m unittest discover -s research/moloch-causal-state-sampling-20261016 \
  -p test_audit_coverage.py -v
```

Synthetic tests create original-shaped files using the actual reviewed
`InputTrace`, `Tail`, `Study`, normal seal and fixed verifier. They cover missing
exchange witnesses without future backfill, stale/gapped/invalid state,
warmup/dedup, original clock identities, timely/late/censored horizons, partial
flow, unsigned/future evidence, pre-end refusal, forged retained replay flags,
source/protocol mutation, spacing and hard identity/scope/enrollment bounds.
Direct kernel bound tests do not assert an independently verified input witness.
No actual V5 capture or economic outcome has been evaluated by these tests.
