# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. This repository does not contain real-money trading assets, credentials, a deployed bot, or a running scheduler.

## Current snapshot

As observed October 1, 2026, **21:26:00–21:26:19 UTC**: **89,820.05 SUSQies cash**, five positions with **10,258.985** remaining FIFO cost, and **no open orders verified at 21:26:00 UTC**. Realized P&L is **79.035** exact (**79.04** rounded), under the documented analytical FIFO aggregate-lot convention. These are historical observations, not live account state.

| Market | Side | Shares | Acquisition prices | Remaining FIFO cost |
|---|---|---:|---|---:|
| Democratic Rhode Island Senate | YES | 6,187 | 1,187 at 0.935 + 5,000 at 0.915 | 5,684.845 |
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.17 |
| Democratic Florida Senate | YES | 7,500 | 0.120 | 900.00 |
| Democratic North Carolina Senate | NO | 1,538 | 0.065 | 99.97 |
| Democratic Alaska Senate | NO | 5,000 | 0.315 remaining lot | 1,575.00 |

The second session bought 5,000 RI YES at 0.915 and sold 3,569 RI YES at 0.950, 5,000 AK NO at 0.330 and 5,000 FL YES at 0.125. One RI cancellation followed a 555-share fill/cancel race; the final 1,187 unfilled shares were canceled. The 555 are included once in total fills. Cash plus exact remaining FIFO cost is 100,079.035; remaining position risk is excluded from realized gain. See the [terminal session audit](data/orders/session_2026-10-01_2102.json), [immutable snapshot](data/snapshots/portfolio_2026-10-01_2126.json), and [full reconciliation](docs/CURRENT_STATE.md).

The [20:36 session audit](data/orders/session_2026-10-01_2036.json) and [20:58 snapshot](data/snapshots/portfolio_2026-10-01_2058.json) remain unchanged. The append-only trade ledger uses negative signed cash cost for sale proceeds; canceled quantities and unsubmitted intents are excluded from trade totals. Exact execution timestamps and unexposed IDs remain unknown.

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
