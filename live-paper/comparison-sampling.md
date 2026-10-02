# Paired comparison observations

The collector can reserve its existing comparison slots for two attempts on the same pair before rotating through the fixed four-market panel. This is a collection treatment, not a new trading signal. It adds no request, changes no order limit, and leaves every frozen strategy bank unchanged.

The measured problem was timing: in a recent 30-minute window, 55 cycles took a median 32.7 seconds and each core market returned roughly every 65.4 seconds. The prospective comparison expires an unfilled entry after 60 seconds. Two-attempt sampling creates more chances for a next-cycle observation, at the cost of a longer interval before the pair returns. It does not guarantee an independent exchange version, an executable fill, or improved returns. Three-cycle bursts were rejected because their longer gaps could repeatedly reset the rebound strategy's 120-second history continuity gate.

Official holdings and pending orders remain first in the scheduler. Existing paper risk and eligible candidates retain their priority. The paired overlay changes only remaining reads labeled `comparison`, after the basket planner; it cannot displace basket, discovery, unknown, or protected reads. A verified reader adds the new comparison wallets' holdings and pending orders as paper risk, allowing a confirmed rebound order to receive a third book without permanently reserving three observation cycles. These hints never create official entry candidates.

The collector reads the named comparison bank and complete derived journal, verifies their identities and hash chain, and incrementally retains the latest holdings and pending orders. A stale worker does not erase inventory. On uncertain or corrupted evidence, it retains the last verified risk and disables optional paired and basket allocation. The local files remain a trust boundary; this is a scheduling hint, not an independent account reconciliation or execution permit.

To activate the reviewed allocation, the operator creates `comparison-sampling.json` in the restricted runtime directory:

```json
{
  "schemaVersion": 1,
  "mode": "paired_comparison_two_attempts_v1",
  "enabled": true,
  "cohort": "entry-common-20261002",
  "bankHash": "the exact frozen cohort bank hash"
}
```

The cohort and hash must match the verified active comparison. Invalid configuration falls back to ordinary allocation. Removing the operating file disables the optional treatment; it does not erase comparison inventory or journal history.

The scheduler durably saves its selected pair, phase, cursor and plan identity before requests. Each member's pre-request reservation is saved once; a failed request or STOP during the shared client's wait can consume that reservation, but never creates an observed price. All first-pass members precede second-pass reservations. Unattempted members remain pending, long carries expire, and a new collector session abandons old carry while preserving rotation. The source journal records the sampling session and distinguishes planned members, reservations and received responses. A response still needs the existing book and source validation before any strategy can use it.

Interpret results by collection regime and retain missing observations. The historical clock-only comparison is a capacity estimate, not a price replay or profit backtest. The same unchanged global read limiter, persistent backoff, official execution ownership and cumulative pilot limits remain in force.
