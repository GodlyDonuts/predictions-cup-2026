# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. This repository does not contain real-money trading assets, credentials, a deployed bot, or a running scheduler.

## Current snapshot

Historical observations on October 1, 2026, **22:21:59–22:22:36 UTC**: cash UI displayed **98,157.74 SUSQies** to two decimals; separate exact quantity × price ledger arithmetic is **98,157.745**. Exact hidden account precision was not exposed. **No open orders** was reverified at **22:22:30 UTC**. This is not live account state.

| Remaining market | Side | Shares | Acquisition price | Remaining FIFO cost |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.170 |

The fourth session fully exited **7,500 Florida YES at 0.105**, with **787.500 SUSQies** proceeds and **−112.500** analytical FIFO realized P&L against **900.000** remaining basis. The one order filled immediately; its ID was not exposed and remains unknown. See the [terminal source audit](data/orders/session_2026-10-01_2219.json), [immutable snapshot](data/snapshots/portfolio_2026-10-01_2222.json), and [full reconciliation](docs/CURRENT_STATE.md).

Cumulative analytical FIFO realized gain is **156.915 SUSQies**. The sole remaining holding has **1,999.170** FIFO basis and retains unrealized/liquidation risk. Arithmetic cash plus remaining cost equals **100,156.915**; this is not current liquidation value. Keep the two-decimal cash display separate from milli-SUSQ ledger arithmetic; do not force equality by rounding individual fills or inventing a hidden balance.

All previous session audits and snapshots remain unchanged and historical, including the [21:32 audit](data/orders/session_2026-10-01_2132.json) and [22:01 snapshot](data/snapshots/portfolio_2026-10-01_2201.json). This update appends exactly one executed aggregate, preserving all 15 prior ledger rows. Earlier canceled Florida/MA orders remain canceled historical orders. Exact execution timestamps and unexposed IDs remain unknown. No bot execution handoff occurs.

Fresh [public-reference research](research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) informed the Florida risk reduction. External prices carry depth, age and settlement limitations; they are not guaranteed fair values or arbitrage.

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
