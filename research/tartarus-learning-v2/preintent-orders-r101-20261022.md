# R101: remove avoidable work before passive-order dispatch

## Installed state,20:17UTC

R101/source8f960c57 is installed as sole worker1131806/startTicks66982381 and
is restoring its original ledger. The engine and original bridge file are
unchanged. Reference helper1131809 waits for current-process trading; finite
file-only observer1131816 is live. No fresh R101 trading, dispatch speedup or
profit is established. [Installation](r101-installed-restoring-20261022.json),
[observation](r101-restoration-observed-20261022.json).

The stopped bank has692 original fill rows. Its extra pre-upgrade fill sold25
NO859 at0.88 for22, zero fees and zero realized gain. Cash33.745; all original
fill rows, official receipts and remaining FIFO are independently reconciled.
[Audit](r101-pre-upgrade-turnover-audit-20261022.json).

## Observed problem

The R100 worker remained live but the latest independently observed native state
still had691 fill rows. Some reserved NO983/NO1077 asks never reached HTTP:
`physical_window_shorter_than_dispatch_budget`. A retained packet's final book
arrived19:48:20.506UTC; the deferral was19:48:29.467UTC. This interval includes
several operations and is not attributed entirely to one function.

A bounded live stack sample caught JSON destruction in
`Cohorts::check -> assess_quantity -> bind_quantity`. Source inspection also
identified a full derived-report publication after the seven final reads and
another on admitting their book, before physical submission. The original report
is about51.8MB. Original event/head and intent durability are separate from
that derived projection.

## Changes

- Borrow exact selected historical order rows through a bounded immutable Arc
  cache during cohort auditing. Its key includes the held Session, manifest and
  actual durable head. Any admitted state change invalidates it; oversized rows
  remain available uncached. Every prior field and ownership check remains.
- Both existing reserved-exit branches now keep one report-projection scope from
  the seven final reads through submission. Original source events, native head,
  cohort binding, intent and network-attempt durability remain synchronous. The
  derived report publishes on successful and error returns, following the
  already-used entry/rank pattern.
- Original receipt clocks, physical deadlines, quote price/quantity/expiry,
  account reconciliation and unknown-acceptance behavior remain authoritative.
  The native library and engine identity are unchanged.

## Evidence and limits

[Allocation benchmark](original-rank-row-sharing-benchmark-20261022.json) uses an
unchanged original51,787,330-byte report, SHA
`2197089562af31c1e61896d7016e5268f8805cab513a818d5e86bbfc577759e0`.
Its26 rank rows encode21,728,236bytes. Eight passes take1.37–1.86seconds with
repeated cloning versus0.18–0.19seconds with shared rows, including their first
copy. Inspected scalar fields match; the entire original JSON remains unchanged.
This is an allocation benchmark, not an end-to-end dispatch or profit result.

[Qualification](r101-qualified-20261022.json):5 order-snapshot cases,
149 fundamental cases,7 rank/cohort cases and271 worker cases pass,432 total.
The new worker case exercises all seven original receipts, durable native head,
unchanged projection through physical-token preparation, unchanged cash/FIFO,
and projection publication on success and error. It separately checks the token's
actual expiry and the packet's earliest receipt deadline. The initial new test
incorrectly equated those deadlines; its failed result is retained on the VPS.

Checked binary SHA:
`40404168cabb1d343bfe676ae54999a780058e5c1cee0e1f9cd23ddba71318fa`.
Build source hashes match the published source after normalizing line endings.
No deployed speed improvement, new fill, profitable strategy or ROI is proved
by these checks. Fresh current-process execution evidence must establish what
the replacement actually changes. Winning and one filled order/minute remain
unmet.

## Operational scope

The replacement preserves the original bank, fill/FIFO history, account queue,
limiter, capital caps, daily windows, engine bridges and selected1077/983 standing
reviews. Only one official account worker may run. The scoped reference helper
continues through that owner after restoration; broader automatic nomination
publishers remain stopped. No strategy or risk expansion is bundled with this
latency change.
