# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. This repository does not contain real-money trading assets, credentials, a deployed bot, or a running scheduler.

## Current snapshot

Historical observations on October 1, 2026, **22:01:18–22:01:47 UTC**: cash UI displayed **97,370.24 SUSQies** to two decimals; separate exact quantity × price ledger arithmetic is **97,370.245**. Exact hidden account precision was not exposed. **No open orders** was verified at **22:01:18 UTC**. This is not live account state.

| Remaining market | Side | Shares | Acquisition price | Remaining FIFO cost |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.170 |
| Democratic Florida Senate | YES | 7,500 | 0.120 | 900.000 |

The third session fully sold **6,187 RI YES at 0.935**, **5,000 AK NO at 0.330**, and **1,538 NC NO at 0.075**. FL 7,500 and MA 2,061 sale orders were canceled entirely unfilled. Actual cancellation submissions were 22:00:12 and 22:00:36, after the 22:00 target. See the [terminal source audit](data/orders/session_2026-10-01_2132.json), [immutable snapshot](data/snapshots/portfolio_2026-10-01_2201.json), and [full reconciliation](docs/CURRENT_STATE.md).

Analytical FIFO realized gain is **190.380** for this session and **269.415 SUSQies cumulative**. The two remaining holdings have **2,899.170** FIFO basis and retain unrealized/liquidation risk. Arithmetic cash plus remaining cost equals 100,269.415; this is not current liquidation value. Keep the two-decimal cash display separate from milli-SUSQ ledger arithmetic; do not force equality by rounding individual fills or inventing a hidden balance.

All previous session audits and snapshots remain unchanged and historical, including the [21:02 audit](data/orders/session_2026-10-01_2102.json) and [21:26 snapshot](data/snapshots/portfolio_2026-10-01_2126.json). The append-only ledger adds only three executed aggregates. Canceled quantities are not trades. Exact execution timestamps and unexposed IDs remain unknown. No bot execution handoff occurs.

Latest research: [Florida/NC conservative scenarios](research/fl_nc_conservative_bands_2026-10-01.md) and [new-race shortlist](research/new_edges_2026-10-01.md). Their subjective bands are not calibrated forecasts or standing execution limits.

Portfolio tournament marks differ substantially from market-detail marks and executable depth. Their formula is unverified. Final rank follows total SUSQies after all markets settle; interim marks are provisional. See the [source-linked valuation review](research/execution_valuation_review_2026-10-01.md).

## Work queue

- [#1 Agent coordination and execution handoffs](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1)
- [#2 Settlement value and portfolio risk research](https://github.com/GodlyDonuts/predictions-cup-2026/issues/2)
- [#3 Read-only API and reconciliation infrastructure](https://github.com/GodlyDonuts/predictions-cup-2026/issues/3)

## Collaboration

dot is the single execution coordinator. Codex starts with research/development only. Both agents can share work through files, issues, and draft pull requests when their respective GitHub connections can access this repository. A new task must explicitly select/read this repo; GitHub does not make either agent watch it continuously.

Repository access and issue comments do not create trading authority. No duplicate orders. Any execution handoff requires an explicit release and matching acknowledgment before the new owner acts; see [coordination protocol](docs/COORDINATION.md).

## Official references

- [Rules](https://predictionscup.com/rules/)
- [API v1 reference](https://sig.thesuper.market/api/v1/docs)
- [Changelog and shared account limits](https://sig.thesuper.market/docs/changelog)
- [Settlement and payouts](https://sig.thesuper.market/docs/settlement-and-payouts)

Trading closes November 4, 2026, noon US Eastern (17:00 UTC), according to the rules reviewed October 1. Recheck source rules for changes.
