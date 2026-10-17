# Moloch — official production bot

Latest research handoff: [October17 current state, Paper7 gaps and next strategy work](docs/MOLOCH_RESEARCH_HANDOFF_20261017.md).
Updated 2026-10-17 03:59:35 UTC: 338 official fill rows, 79,347.690 SUSQies owned cost and 16,092.385 cash. Release a557c779 / 7d44e839, worker 607949, is verified trading at 03:57:59 UTC with healthy realtime. No new post-upgrade fill or measured performance gain is claimed.
It separates actual production/fills, disabled strategies and missing economics.
Infrastructure work is deferred unless essential.

For new research, start with the [current Moloch research baseline](docs/MOLOCH_RESEARCH_BASELINE.md), then the latest dated [production evidence](docs/CURRENT_STATE.md). The baseline separates deployed code, checked source, disabled features and unfinished work. Historical sections below are not the current deployment specification.

Moloch is the human-selected production system for the 2026 Susquehanna Predictions Cup. Its Rust runtime is in [moloch/live-runtime](moloch/live-runtime/README.md), with the exact financial ledger and replay engine in [moloch/native](moloch/native/RUST_MIGRATION.md). The existing VPS service is `moloch-live.service`; deployment and actual fill/profit observations are recorded in [CURRENT_STATE](docs/CURRENT_STATE.md) and [execution task #120](https://github.com/GodlyDonuts/predictions-cup-2026/issues/120).

The human directed Moloch to take over `main` and the primary checkout. Useful Baal transport, account reconciliation, risk utilities and research remain in the repository; there is one production account worker. All-market observation does not imply simultaneous exposure in every market. Actual fills and realized profit must come from official receipts and the attributed ledger, not public prints or simulations.

See [the source integration and retained components](docs/MOLOCH_MAIN_TAKEOVER_20261015.md). The dated Baal sections below are historical and do not select the current runtime.

# Baal current status — October7

Baal's capital allocation is now10,000 SUSQies, with100/order,1,000/market and2,000/race limits. The first larger official trade bought100NOshares for9.000SUSQies and is fully reconciled; new realized profit is0.000 while the position remains open. The previous complete scaled cohort closed at+.780, including its losses. Current deployment and source-bound evidence are in [CURRENT_STATE](docs/CURRENT_STATE.md). Older dated sections below are historical.

# Baal current status — October6

Current deployment, attributed official fills and remaining work are recorded in [CURRENT_STATE](docs/CURRENT_STATE.md). Baal is the sole execution owner. The generic live strategy has four closed current-pilot trades netting+0.015SUSQies, with no current pilot holdings; the new1,000SUSQie allocation is live and its six purchases and eight verified profitable exits have realized0.975SUSQies;68shares remain across three markets; the original buy window has closed and owned exits remain managed, zero fees, a tiny sample. Historical dated reports below do not describe the current runtime.

# October5: accelerated paired-contract paper and Rust transport

Baal now runs two separately frozen Michigan Governor dual-NO paper policies inside the single collector. Full two-leg funding, later receipts, observed depth, zero fees and failed-hedge accounting are enforced. The original cohort holds a0.975 package with zero realized profit; its0.025 ordinary settlement surplus is conditional and not cash profit. Virtual-clock replay processes81.5 captured hours in about9.7seconds while preserving coverage gaps and exact original journal parity.125 correctness checks pass. Contrary Texas polls now block new fundamental entries, and two expired flat hosts are retired with accounting preserved. No contest activation was renewed. See [the measured sprint results](docs/PROFIT_SPRINT_RESULTS.md) for results and limitations.

# October5: funded fundamental-value paper wallet

A separate future-cutoff wallet now runs the existing Rust settlement-value policy inside the single collector. It uses Governor-specific historical error with the original primary-source clocks, later-request one-share fills, zero fees and exact funded accounting. It keeps positions according to current fundamental value rather than a two-day timeout or price-only stop. The original research/bulk banks and journal prefixes are preserved; the bulk entry drain remains in force.115 correctness checks pass. The original contest activation remains expired, and profitability is not established. See [implementation and economic limits](docs/FUNDED_SETTLEMENT_PAPER.md) and the subsequent paper evidence in data/funded-settlement-paper-live-20261005.json.

# October5: independently sourced Texas governor paper experiment

New unowned bulk quotes are now draining with durable operator boundaries; prior liabilities and inventory remain. Texas produced its first subsequent one-share paper fill at0.295, with zero closed trades and−0.015 estimated liquidation P&L at19:24UTC. Governor-specific history now measures231 races/800 observations and178 earlier-year-only diagnostic forecasts; it is a later candidate, not an overwrite of the active bank or proof of market profit. A new frozen one-share Texas Democratic governor cohort uses three primary polls, including the contrary Emerson result, an explicitly unvalidated and widened governor error proxy, and only subsequent executable receipts. It shares the existing paper worker and four book slots; prior Kansas/West Virginia holdings, banks and journal prefixes remain separate. No profitable result or official activation is claimed. See [source inputs, sensitivity and execution limits](research/current-polls-v3/README.md).

# October5: fill-conditional models and funded quote replay

Quote-placement features, separate arrival/filled-value models, and an exact shared-capital replay now expose adverse-selection losses and unresolved quote liabilities. Both join-price and inside-price selectors choose zero later trades; exact-exit sensitivity reduces losses but leaves them negative. The funded inside-price development control realized−0.040 while still holding0.925 cost and2.060 unresolved reservations. No profitable strategy or official activation is claimed. See [measured results](docs/QUOTE_OUTCOME_RESULTS.md).

# October5: fresh execution receipts restored; longer horizons show no qualified edge

A depth200 substitution in the last collector change invalidated the frozen depth20 publisher. That regression is corrected and checked against the actual native publisher. At17:54UTC collector8616, paper3976 and dashboard20092 retained the original banks/accounting and one account queue; the overdue spread position closed at−0.020paper SUSQies. Kansas showed+0.005estimated liquidation P&L and the older research wallet+0.025including its prior+0.020realized gain; correlated latency copies must not be summed. Neither the bulk wallet nor the supported5/30-minute forecasts established new profitable execution. The fixed two-hour purged comparison has insufficient chronology. No contest activation was extended or order submitted. See [the measured results and regression recovery](docs/LONGER_HORIZON_RESULTS.md).

# October5: market-wide bulk history and inside spread quotes

October5: a separately frozen coupled paper cohort now uses causal bulk-price history across all237 exchanges and one-tick-inside paired quotes. Fresh full-book depth and account eligibility still control quotes and exits. Existing read slots and wallet histories are preserved. The original-source replay processed21,813 events in39.52s but had no fills and missing quote-window tape coverage; no profit is claimed. The 87 integration checks pass. The original official activation remains expired. See [implementation and measured limits](docs/COUPLED_BULK.md).

# October5: shared-wallet spread bot and repaired collection

October5: repaired tape selection and started a shared-wallet spread cohort. The full237-exchange tape scope was already correct; the runner incorrectly selected only its old four markets. Exact recovery receipts for364/376 resolved all four blocked quotes without fills. A new separately frozen coupled YES/NO paper wallet models the documented immediate one-SUSQie pair payout, retains unmatched inventory risk, and uses zero fees. Historical original-source replay processed7,567 events in23.64s: one hypothetical purchase, zero completed pairs and zero realized profit. No production activation or profit is claimed. Windows sharing-denial retries and preserved polling-code archives address actual collector/restart failures. See [current implementation and measured results](docs/COUPLED_SPREAD.md).

# October 5: broader passive cohort and actual loss attribution

A new independently frozen16-market passive paper cohort now shares the same collector, after original-source screening of237 exchanges. The narrow cohort's pending quotes were fully reconciled before retirement, with its bank/journal preserved. Actual prior bot loss is−0.380: two losing markets cost0.500 and winning markets earned0.120. No new official orders or demonstrated profitability are claimed. [Execution economics and implementation](docs/EXECUTION_ECONOMICS.md). Older cohorts and timestamps below remain historical.

# October 5: stable spread paper implementation

The losing price-entry engines are retired. A new frozen passive paper cohort joins stable prices, seeks a one-tick covered exit and reconciles strict anonymous-trade projection windows. It runs inside the existing collector, with zero fees and no extra services. Spare reads now build continuous history; existing official and source-probability risk retains priority. **No profitability is established and no new official orders were sent.** See [implementation and measured evidence](docs/STABLE_SPREAD.md). Older timestamps and results below remain historical.

# October5: paper exits now have a matching native production policy

The new separately built receipt policy matches one-share paper take-profit, stop, horizon, conservative prices and cooldown, with durable risk exits and idempotency. It is a checked production candidate, not an activated strategy. The actual12:48UTC native diagnostic still showed Kansas paper liquidation P&L−0.015 and no realized profit. See [implementation and evidence](execution/receipt-production.md). The original official entry window remains closed.

# Current production status — October 5

One new empirical polling paper strategy is active for Kansas market356 YES, sharing the same collector and worker. Its first counterfactual fill is0.345; at12:16UTC it had no closed trade and−0.015 depth-aware liquidation P&L. This is not profitable production evidence. [Model and strategy](research/poll-calibration-v1/README.md) retain measured historical errors, source clocks and visible limitations. Current work is profitability and necessary trading correctness.

Baal remains the human-assigned sole execution owner. The bounded pilot's original entry window expired at **2026-10-05T03:59:22.131Z**; new production entries are closed. Recovered collector, paper worker and dashboard preserve the same activation, frozen bank and accounting. This pilot has submitted **zero orders** and has **zero realized P&L**; profitability is not established. A controller correction now displays the expired window explicitly. See [current state](docs/CURRENT_STATE.md), [recovery evidence](data/production-recovery-20261005.json) and [new primary polling assessments](research/current-polls-v1/README.md). Earlier operational snapshots retain their historical timestamps.

# Current execution authority — October 4

Codex/Baal is the sole execution owner directly assigned by the human, with assignment [5984685670](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5984685670) and own ACK refresh [5984845412](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5984845412). The bounded zero-fee settlement pilot is live:1 SUSQie/order,10 aggregate attributed cost,25 cumulative purchases,3 markets, entries expire2026-10-05T03:59:22.131Z. No current qualifying forecast, order or profitability is claimed by this documentation. See coordination/execution_state.json, execution/production-launch.md and the live dashboard. Daenerys is advisory; her current human-directed job-application priority remains.

# Historical superseded execution authority — October 3

The human reassigned contest trading to Daenerys until Codex has a successful production bot. Codex remains responsible for quantitative engineering and paper testing; its local contest writer is explicitly disabled. RELEASE `codex-to-daenerys-20261003-0725` is recorded in [issue comment 5966759363](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966759363); incoming ACK and fresh whole-account reconciliation are required. Earlier role assignments below are historical and superseded. Return to Codex requires a named stop/RELEASE/ACK and fresh reconciliation, not a passing test or paper signal.

# Predictions Cup 2026

Private shared workspace for the [Susquehanna Predictions Cup](https://sig.thesuper.market/): research, a reconciled simulated trade ledger, and coordination between dot and Codex.

## Start here

1. Read [AGENTS.md](AGENTS.md) before working.
2. Check [current state](docs/CURRENT_STATE.md) and the timestamped [portfolio snapshot](data/portfolio/latest.json).
3. Read [strategy and open questions](docs/STRATEGY.md).
4. Claim work and post updates in [coordination issue #1](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1).

The challenge uses free, nonredeemable SUSQies. No real-money trading assets or credentials are stored here. The new bounded official competition executor is described in [execution/README.md](execution/README.md); activation and journals stay in restricted local runtime storage.

## Current snapshot

Historical observations on October 2, 2026, **06:06:14–06:06:57 UTC**: cash UI displayed **93,532.74 SUSQies** to two decimals; separate exact quantity × price ledger arithmetic is **93,532.745**. Exact hidden account precision was not exposed. **No open orders** was verified at **06:06:57 UTC**. This is not live account state.

| Remaining market | Side | Shares | Acquisition price | Remaining FIFO cost |
|---|---|---:|---:|---:|
| Democratic Massachusetts Senate | YES | 2,061 | 0.970 | 1,999.170 |
| Democratic Rhode Island Senate | YES | 5,000 | 0.925 | 4,625.000 |

Exactly one new executed aggregate: **BUY 5,000 RI YES at 0.925**, cost **4,625.000**, submitted approximately **05:45:48** and verified **05:46:44 UTC**. The buy order ID remains unknown. RI exit **1513474 at 0.955** and MA exit **1497695 at 0.975** were both **canceled with zero fills**; neither creates a sell ledger row. All **16 prior ledger rows** are preserved byte-for-byte, for **17 executed aggregates** total.

Cumulative analytical FIFO realized gain remains **156.915 SUSQies**. Remaining FIFO basis is **6,624.170**; arithmetic cash plus basis is **100,156.915**, not current liquidation value. Keep the two-decimal cash display separate from milli-SUSQ ledger arithmetic; do not invent a hidden balance or rounding adjustment.

RI's final UI marked value/P&L was **4,604.50 / −20.51**; MA's was **1,975.37 / −23.80**. These are marks, not fills. The RI bid at **06:06:14** was **0.900 × 40,855**, sufficient for a historical gross **4,500.000** liquidation scenario and **−125.000** versus RI basis before fees or quote movement. No sale at that bid occurred; whole-account liquidation value is unknown without contemporaneous MA depth.

See the [RI derived audit](data/orders/session_2026-10-02_ri_value.json), [MA derived audit](data/orders/session_2026-10-02_ma_exit.json), [cancellation summary](data/orders/cancellations_2026-10-02_0606.json), [immutable snapshot](data/snapshots/portfolio_2026-10-02_0606.json), [full-ledger replay and source/projection hashes](data/snapshots/reconciliation_2026-10-02_0606.json), and [full reconciliation](docs/CURRENT_STATE.md). All previous audits and snapshots remain unchanged and historical. That historical account update did not transfer bot ownership; the human reassigned ownership later, as recorded below.

Research context: [official UNH abstract](https://scholars.unh.edu/survey_center_polls/1009/) and [public Kalshi book observed at 05:41:04 UTC](data/snapshots/ri_public_book_2026-10-02_0540.json). External prices have depth, age and settlement limitations; they are not guaranteed fair values, booked profit or risk-free arbitrage. The earlier [Florida risk-reduction research](research/public_reference_2026-10-01/public_reference_report_2026-10-01.md) remains historical.

Latest research: [Florida/NC conservative scenarios](research/fl_nc_conservative_bands_2026-10-01.md) and [new-race shortlist](research/new_edges_2026-10-01.md). Their subjective bands are not calibrated forecasts or standing execution limits.

Portfolio tournament marks differ substantially from market-detail marks and executable depth. Their formula is unverified. Final rank follows total SUSQies after all markets settle; interim marks are provisional. See the [source-linked valuation review](research/execution_valuation_review_2026-10-01.md).

## Work queue

- [#1 Agent coordination and execution handoffs](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1)
- [#2 Settlement value and portfolio risk research](https://github.com/GodlyDonuts/predictions-cup-2026/issues/2)
- [#3 Read-only API and reconciliation infrastructure](https://github.com/GodlyDonuts/predictions-cup-2026/issues/3)

## Collaboration

**Current ownership, October 3:** Daenerys / dot is the temporary sole execution owner under the human's 06:48:26 UTC instruction and 06:53:24 UTC confirmation that the bot is inactive and the PC is off. [Acknowledgment and scope](https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5966533944); handoff `human-20261003T064826Z-codex-to-daenerys`. Codex / Drogon must keep account writes and collectors paused, including on restart, until a new coordinated return handoff after the human says the bot is active. Old activations must not resume automatically.

The PC-off/bot-inactive state is human-reported, not a remotely verified process stop or shared-lock/lease proof. This documentation change does not control a runtime. Resting orders may still fill; fresh account reconciliation is required before any action. Prior pilot observations remain historical.

### Historical October 2 assignment

The human explicitly appointed Codex sole execution owner on October 2. Daenerys / dot is an advisory researcher only and confirmed her account worker stopped with no in-flight ambiguity (issue comment 5947535773). Both agents share work through files, issues, and tested direct commits on main when their respective GitHub connections can access this repository. The human requested direct main pushes; do not create PRs. A new task must explicitly select/read this repo; GitHub does not make either agent watch it continuously.

Repository access and issue comments do not create trading authority. No duplicate orders. The direct human reassignment supersedes the original dot RELEASE prerequisite. Any subsequent agent-initiated transfer still requires an explicit release and acknowledgment; see [coordination protocol](docs/COORDINATION.md).

## Official references

- [Rules](https://predictionscup.com/rules/)
- [API v1 reference](https://sig.thesuper.market/api/v1/docs)
- [Changelog and shared account limits](https://sig.thesuper.market/docs/changelog)
- [Settlement and payouts](https://sig.thesuper.market/docs/settlement-and-payouts)

Trading closes November 4, 2026, noon US Eastern (17:00 UTC), according to the rules reviewed October 1. Recheck source rules for changes.

## Historical production-pilot milestone (October 2)

The first production pilot is bounded to 25 SUSQies per order, 100 aggregate attributed cost, 100 cumulative purchases and three markets. It requires qualified signals, durable immutable idempotency, fresh reconciliation, an exclusive writer and zero added fees. Test results establish correctness, not profitability. The local Next.js terminal at http://127.0.0.1:3000 separates official account trades, bot attribution and independent paper wallets. See [execution documentation](execution/README.md).

## Historical quant development status

- `bot/`: GET-only scanner, with STOP checked before credentials or network use.
- `paper/`: offline replay, conservative simulated fills, exact accounting and risk tests.
- `feed/`: offline collector and durable sanitized capture contract.
- `ml/`: offline order-book baseline and time-split evaluation on synthetic fixtures.
- `capture-live/`: bounded authenticated transport with injected SDK/budget interfaces and regression tests; not deployed or connected.

The local quant runner now paper-trades fresh official API data using hypothetical taker fills in a separate bankroll. These simulated fills are not actual contest trades or evidence of profitable strategy performance. Synthetic fixture results remain separate. Codex owns contest execution; prior account holdings remain separate from bot-attributed inventory.

Aggregate offline validation on Windows: 216 selected tests passed. Two additional POSIX-specific tests fail here (Unix mode 0600 and graceful child SIGTERM); neither Unix permissions nor graceful SIGTERM behavior is verified on Windows. This is correctness testing with synthetic inputs, not strategy performance evidence.

## Working paper runner

**Paused under the October 3 reassignment:** the commands below are retained as documentation, not permission to restart collectors or execution before the new return handoff.

See [live-paper/README.md](live-paper/README.md). Run `./Start-Paper.ps1` on the approved Windows host, open http://127.0.0.1:8787/ for stats, and use `./Stop-Paper.ps1` to stop. The dashboard shows the rolling-mean strategy, small buy-and-hold control, no-trade baseline, realized/marked/liquidation P&L, fees, drawdown, signals and collection skips. Replay saved captures without network calls to compare parameters.99 relevant tests pass; actual GET capture, hypothetical control fills, dashboard, restricted Windows runtime ACLs, graceful stop and journal-backed resume were checked locally on October1. The collector remains read-only unless a valid local competition activation is present; the reviewed executor then shares the same serialized API queue.
