# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. This repository does not contain real-money trading assets, credentials, a deployed bot, or a running scheduler.

## Current snapshot

As observed October 1, 2026, **20:57:59–20:58:30 UTC**: **88,729.50 SUSQies cash**, five positions with **11,270.50** remaining FIFO cost, and **no open orders verified at 20:58:06 UTC**. Realized P&L is **0.00**. These are historical observations, not live account state.

| Market | Side | Shares | Average entry | Remaining cost |
|---|---|---:|---:|---:|
| Democratic Rhode Island Senate | YES | 4,756 | 0.935 | 4,446.86 |
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.17 |
| Democratic Florida Senate | YES | 12,500 | 0.120 | 1,500.00 |
| Democratic North Carolina Senate | NO | 1,538 | 0.065 | 99.97 |
| Democratic Alaska Senate | NO | 10,000 | 0.32245 (aggregate) | 3,224.50 |

The 20:36 session sold the original 2,072 RI YES at cost, bought 4,756 RI YES at 0.935 and 5,000 AK NO at 0.315, and canceled every remaining quote. AK retains two acquisition lots. See the [terminal session audit](data/orders/session_2026-10-01_2036.json), [immutable snapshot](data/snapshots/portfolio_2026-10-01_2058.json), and [full reconciliation](docs/CURRENT_STATE.md). The trade ledger uses negative signed cash cost for sale proceeds; canceled unfilled quotes are excluded from trade totals.

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
