# Moloch turnover release — October 16, 22:34 UTC

Production PID **565108** is running the optimized ARM64 release from
`20543435f9354ae5a599866545cbf1aae6a03cfa`, binary
`d9f5cbc628dd2e2d2d4f970106706029b8af56b723b5ac51f80ac1939ab48b3d`,
native engine `b5b28a476fc057de93f5784f7b3600116418aa7e775f25576cfd2906ecd77713`.
The initial verification confirms the actual executable and no STOP, but history
restoration is still underway. **Fresh trading and post-upgrade fills are pending.**
[Original deployment evidence](../research/moloch-turnover-deployment-20261016/README.md).

## Trading changes

- Fixed a confirmed activation failure: Alaska/Maine native reservations remained
  valid, but a consumed planning-packet deadline expired during activation and
  blocked the final fresh-facts phase. Post-reservation checks now use the genuine
  reservation/review deadlines, then obtain the existing new seven-receipt
  execution packet. Pre-reservation planning freshness remains enforced.
- Enabled cumulative profitable-depth ordinary SELL proposals. They use actual
  own-side marginal bid depth, worst owned-lot basis, declared fees and minimum
  gain, with the existing 50% displayed-depth allowance. Covered inventory,
  pledges and opposing acquisition obligations bound quantity. This does not
  turn a mark or entire top-of-book position into an executable exit.
- Added explicitly reviewed same-cohort top-ups: all prior physical entries must
  have exact original closure and fill attribution; total held-plus-new cost and
  quantity remain bounded. Unknown writes, inherited inventory, opposing/package
  inventory and cohorts that began exiting cannot be treated as new entry capital.
- Indexed already closed, verified ordinary ACKs at startup instead of repeatedly
  cloning/reconciling the whole engine for them. Open/fundamental/package cases
  retain their original paths. No measured startup speedup is claimed yet.

## Active model limits

| Exchange | Model | Aggregate principal | Total cohort shares | Physical entries |
|---|---|---:|---:|---:|
| 977 | Oregon Republican NO | 5,000 | 10,000 | 2 maximum |
| 1067 | Alaska Republican YES | 3,000 | 10,000 | 2 maximum |
| 865 | Maine Democratic YES | 3,000 | 10,000 | 2 maximum |

These are operator risk limits, not Kelly estimates or calibrated probability
bounds. Every forecast point, original source byte/hash, publication/observation
clock, expiry, contract mapping, exception assumption and terminal objective is
preserved. Delaware/Massachusetts/NC review entries are exact unchanged. The
authority appends three new review digests while retaining all old ones.
Source expiry blocks new entries; it does not force a terminal holding sale.
No old source timestamp was renewed by changing the principal budget.

Only `activation.binarySha256`, `fundamentalPolicy` and `profitableDepthMode`
changed in Control. Full effective account capital remains **95,579.915000**,
with the same original bank, queue, daily windows and account-wide limiter.
The implementation does not promise that displayed opportunities will fill.

## Original history and validation

The genuine frozen **308,166-event / 952,440,605-byte** original history replay
passed with byte-identical public engine state and original financial files.
The completed original baseline was reused, so this is financial parity rather
than a fresh paired speed comparison.

The final production boundary is **348,466**, with **241** cumulative fill rows.
The SDK had zero unknowns, no active request and no manual-review flag; original
ordinary dispatch attempts were resolved. The actual stopped-worker binary was
bound into a thirteenth bridge; all twelve previous entries are exact preserved.
The root-owned controlled STOP was archived under its original unique request.
No financial event, fill, cost, source clock or contribution was rewritten.

Actual ARM64 validation: runtime212, worker106, fundamental96, value-surface8,
native76 and profitable-depth14 pass; one existing native benchmark is explicitly
ignored. Fundamental96 uses corrected fixture `a40888d7` against the exact
unchanged205 production libraries. The original failing fixture log remains in
the evidence bundle. Release build and the actual engine-identity probe pass.

At the final old-worker boundary: cash **70,762.850000**, owned cost
**24,680.835000**, new-trading realized **−181.360000**, inherited gross
**+45.130000**, combined realized **−136.230000**. These are attributed ledger
figures, not the SIG dashboard's daily valuation P&L. Overall profitability is
not established. A new PID is not evidence of new trading.

## Paper 7 is a separate candidate

Joint maker qualification, fundamental acquisition collection and own-account
realtime invalidation are integrated locally with defaults off. The first
native90/runtime222/worker109 checks pass; maker fixture corrections and remaining
integration/release checks are underway. They are **not in the production205
binary**. No empirical maker model, correlated posterior or tournament rank
strategy is qualified. See [Paper 7](MOLOCH_PAPER7_20261016.md).
