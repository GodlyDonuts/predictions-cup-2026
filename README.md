# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. This repository does not contain real-money trading assets, credentials, competition order execution or runtime credentials. An optional local read-only paper runner is available below.

## Current snapshot

Historical observations on October 2, 2026, **06:06:14–06:06:57 UTC**: cash UI displayed **93,532.74 SUSQies** to two decimals; separate exact quantity × price ledger arithmetic is **93,532.745**. Exact hidden account precision was not exposed. **No open orders** was verified at **06:06:57 UTC**. This is not live account state.

| Remaining market | Side | Shares | Acquisition price | Remaining FIFO cost |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.170 |
| Democratic Rhode Island Senate | YES | 5,000 | 0.925 | 4,625.000 |

Exactly one new executed aggregate: **BUY 5,000 RI YES at 0.925**, cost **4,625.000**, submitted approximately **05:45:48** and verified **05:46:44 UTC**. The buy order ID remains unknown. RI exit **1513474 at 0.955** and MA exit **1497695 at 0.975** were both **canceled with zero fills**; neither creates a sell ledger row. All **16 prior ledger rows** are preserved byte-for-byte, for **17 executed aggregates** total.

Cumulative analytical FIFO realized gain remains **156.915 SUSQies**. Remaining FIFO basis is **6,624.170**; arithmetic cash plus basis is **100,156.915**, not current liquidation value. Keep the two-decimal cash display separate from milli-SUSQ ledger arithmetic; do not invent a hidden balance or rounding adjustment.

RI's final UI marked value/P&L was **4,604.50 / −20.51**; MA's was **1,975.37 / −23.80**. These are marks, not fills. The RI bid at **06:06:14** was **0.900 × 40,855**, sufficient for a historical gross **4,500.000** liquidation scenario and **−125.000** versus RI basis before fees or quote movement. No sale at that bid occurred; whole-account liquidation value is unknown without contemporaneous MA depth.

See the [RI derived audit](data/orders/session_2026-10-02_ri_value.json), [MA derived audit](data/orders/session_2026-10-02_ma_exit.json), [cancellation summary](data/orders/cancellations_2026-10-02_0606.json), [immutable snapshot](data/snapshots/portfolio_2026-10-02_0606.json), [full-ledger replay and source/projection hashes](data/snapshots/reconciliation_2026-10-02_0606.json), and [full reconciliation](docs/CURRENT_STATE.md). All previous audits and snapshots remain unchanged and historical. No bot execution handoff occurs.

Research context: [official UNH abstract](https://scholars.unh.edu/survey_center_polls/1009/) and [public Kalshi book observed at 05:41:04 UTC](data/snapshots/ri_public_book_2026-10-02_0540.json). External prices have depth, age and settlement limitations; they are not guaranteed fair values, booked profit or risk-free arbitrage. The earlier [Florida risk-reduction research](research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) remains historical.

Latest research: [Florida/NC conservative scenarios](research/fl_nc_conservative_bands_2026-10-01.md) and [new-race shortlist](research/new_edges_2026-10-01.md). Their subjective bands are not calibrated forecasts or standing execution limits.

Portfolio tournament marks differ substantially from market-detail marks and executable depth. Their formula is unverified. Final rank follows total SUSQies after all markets settle; interim marks are provisional. See the [source-linked valuation review](research/execution_valuation_review_2026-10-01.md).

## Work queue

- [#1 Agent coordination and execution handoffs](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1)
- [#2 Settlement value and portfolio risk research](https://github.com/GodlyDonuts/predictions-cup-2026/issues/2)
- [#3 Read-only API and reconciliation infrastructure](https://github.com/GodlyDonuts/predictions-cup-2026/issues/3)

## Collaboration

dot is the single execution coordinator. Codex starts with research/development only. Both agents share work through files, issues, and tested direct commits on main when their respective GitHub connections can access this repository. The human requested direct main pushes; do not create PRs. A new task must explicitly select/read this repo; GitHub does not make either agent watch it continuously.

Repository access and issue comments do not create trading authority. No duplicate orders. Any execution handoff requires an explicit release and matching acknowledgment before the new owner acts; see [coordination protocol](docs/COORDINATION.md).

## Official references

- [Rules](https://predictionscup.com/rules/)
- [API v1 reference](https://sig.thesuper.market/api/v1/docs)
- [Changelog and shared account limits](https://sig.thesuper.market/docs/changelog)
- [Settlement and payouts](https://sig.thesuper.market/docs/settlement-and-payouts)

Trading closes November 4, 2026, noon US Eastern (17:00 UTC), according to the rules reviewed October 1. Recheck source rules for changes.

## Quant development status

- `bot/`: GET-only scanner, with STOP checked before credentials or network use.
- `paper/`: offline replay, conservative simulated fills, exact accounting and risk tests.
- `feed/`: offline collector and durable sanitized capture contract.
- `ml/`: offline order-book baseline and time-split evaluation on synthetic fixtures.
- `capture-live/`: bounded authenticated transport with injected SDK/budget interfaces and regression tests; not deployed or connected.

The local quant runner now paper-trades fresh official API data using hypothetical taker fills in a separate bankroll. These simulated fills are not actual contest trades or evidence of profitable strategy performance. Synthetic fixture results remain separate. dot retains contest execution ownership.

Aggregate offline validation on Windows: 216 selected tests passed. Two additional POSIX-specific tests fail here (Unix mode 0600 and graceful child SIGTERM); neither Unix permissions nor graceful SIGTERM behavior is verified on Windows. This is correctness testing with synthetic inputs, not strategy performance evidence.

## Working paper runner

See [live-paper/README.md](live-paper/README.md). Run `./Start-Paper.ps1` on the approved Windows host, open http://127.0.0.1:8787/ for stats, and use `./Stop-Paper.ps1` to stop. The dashboard shows the rolling-mean strategy, small buy-and-hold control, no-trade baseline, realized/marked/liquidation P&L, fees, drawdown, signals and collection skips. Replay saved captures without network calls to compare parameters.99 relevant tests pass; actual GET capture, hypothetical control fills, dashboard, restricted Windows runtime ACLs, graceful stop and journal-backed resume were checked locally on October1. This runner submits zero competition orders.
