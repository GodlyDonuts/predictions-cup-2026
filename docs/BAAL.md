# Baal

Baal is the human's chosen bot name. The local Next.js dashboard at http://127.0.0.1:3000 displays the official account separately from counterfactual paper wallets. Naming changes do not rename historical strategy banks or change execution ownership.

As of October 4, 06:30 UTC, the improved-quote maker wallet had one completed modeled round trip: buy one NO at 0.200, sell at 0.185, realized loss 0.015, cash 99.985. The source audit binds the buy to an anonymous print and the sell to captured executable depth after an adverse tick. The join-quote wallet had no fills. These are local counterfactual fills, not competition executions or profitability evidence. Added fees are zero; spread, delay, depth and adverse prices still matter.

Maker host V3 and core V2 correct two reviewed accounting defects: absence of a qualifying trade requires contiguous fully paginated coverage of the quote's active interval with sufficient projection checkpoints; rejected account/stale books cannot supply liquidation marks. Incomplete tape windows restart from their original first page instead of advancing past missing coverage. The paired admission host considers both actual books only once the coherent pair is available at the consumer clock.

V1 and V2 code, banks and journals are preserved. V3 continues V2's exact wallet cash, fills, inventory and pending quotes through a bank-bound opening-state snapshot and verified predecessor journal prefix. It cancels modeled quote intervals at the transition, clears old marks/coverage and preserves late pre-cancel print handling. Its totals include inherited accounting; predecessor and successor balances must never be added or represented as independent new profits. The original probability wallets also remain unchanged.

The three persistent services are one read-only collector, the original probability paper process and the Next.js dashboard. The collector retains its coordinated 20-read/minute allocation, serialized admission and persistent backoff. Codex competition activation remains disabled; Daenerys remains the contest execution owner under the existing named handoff. No new contest orders were submitted for this milestone.

The paired V2 paper engine uses two causal, sequential acquisitions with fixed package price caps. It walks captured depth past a best rung too small at the one-percent participation limit. A failed hedge or second sale remains owned inventory, with exact cash/FIFO accounting and bounded orphan exits. Conditional ordinary settlement surplus is separate from realized and liquidation P&L. Its opening state continues V1 accounting. At 06:30 it had zero fills and 100 cash. The dashboard's **Baal · paired inventory** selector shows this wallet.

The collector's paired/risk reads replace optional slots, preserving protected obligations and the original read allocation. Active maker tape service uses three auxiliary tape cycles followed by one history cycle, with persistent fair discovery. A short pre-interruption sample reduced NH tape receipt medians from approximately 286 seconds to approximately 142 seconds; this is a service measurement, not a return estimate. A subsequent overlapping entry/exit study had 87 anchors with later complete depth and zero positive resale marks. It does not establish profitable turnover.

Timing and portfolio-selection work from commits `ab37b51` and `2b7fc723` is integrated. Before changing shared transport, six existing paper banks' exact source graphs were preserved in the restricted runtime's `frozen-code` directory. Bank-bound source validation lets the collector use current transport while existing wallets retain original reducers and accounting. This private snapshot must accompany runtime recovery; a missing or changed dependency fails identity validation. Historical Git sources and preservation receipts remain available.

Maker V4 is a tested precursor to the active V5 correction described below. Both its preparation and host reject a migration whose sealed tape prefix is ahead of the predecessor's consumed frontier. The actual V2-to-V3 transition matched sequence 91 and its exact hash, so the reported skipped-page case did not affect that transition. The original NH review expired at 04:36:16 UTC; the successor uses separately captured source evidence with its own expiry.

Validation for the combined iteration: 86 focused checks passed, zero failures; production Next.js build and browser verification; live PID/lease, frozen-source/prefix, migration-frontier and dashboard-accounting checks. This is a bounded test run, not a claim that the entire repository suite is green. Synthetic cases establish correctness only. Remaining work includes fresh rule evidence, subsequent economic measurements and compact training capture for the growing collector journal.

October4 fast replay: the complete original4,255,286,972-byte capture is verified and replayed in receipt order without future observations or rewritten timestamps.128short-horizon models/384wallets and128long-horizon models/384wallets both selected no-trade. The short comparison saw1642eligible observations per wallet,415warmup and1227below its fixed edge margin. Four-worker replay measured279.65→138.53seconds including source verification, with exact model weights and wallet metrics. This is2.02x for this sample, not10x or profitability.63focused checks passed. Native Rust replay/compute migration is the current human-requested next implementation. See data/baal-fast-replay-milestone-20261004.json.

October4 Rust backend: native original-prefix verification, frozen ridge/stump inference and integer wallet replay are implemented. The live Node collector/dashboard and existing fitting remain unchanged. Same-weight384wallet replay matched all1152JS checkpoints and measured127.73seconds JS (4workers) versus12.21seconds Rust (8),10.46x in this no-refit run. A2304variant dip grid processed the entire2.31day capture in25.62seconds including verification, with zero fees and modeled spread/slippage/depth/latency. Nine correlated historical variants ended at+0.005 on a100-unit wallet after five closes; most lost. This is development evidence, not demonstrated profitability or production activation.36Node checks plus one Rust exact-money check passed. See backend-rust/README.md and data/baal-rust-milestone-20261004.json.

October 4 native economics: a fresh-wallet subsequent comparison processed 100 exit/cost cards over 294 newer records, 06:48–09:07 UTC. All had zero fills and zero P&L. Separate warmup, flat-history, z and margin diagnostics show no cost-qualified dip. The prices existed before the declaration, so this is retrospective subsequent data, not live prospective trading. The original result and its diagnostic successor remain immutable.

The separate development-cost study simulated 180 cards over 2.31 days in 1.06 seconds after 11.53 seconds of original-byte verification. Eighteen historical baseline wallet totals match the original grid exactly. For the selected five-close baseline, midpoint gain of 0.0825 minus 0.0275 spread/ladder cost and 0.0500 slippage left only 0.0050; fees remain zero. Two-tick slippage made it negative, and the experimental profit-floor/bounded-stop exit did not improve it. These findings reject scaling or deployment of that selected dip variant. The native source, binary, declarations, fills and receipts are preserved; no contest orders or extra persistent processes were added. Thirteen native Node checks and one Rust check passed; adjacent JavaScript checks previously passed, not a claim that the full repository suite is green. See `data/baal-native-evaluation-20261004.json` and `npm run evaluate:rust`.

The source-probability paper V1 recorded its first completed round trip, buying one share at 0.905 and selling at 0.925 on later independent requests, for +0.020 after modeled one-tick slippage and zero fees. The 1s and 3s cases share the same opportunity. Another paper share remained held with invalidated marks because the official account also holds that exchange. Consequently the whole-wallet P&L was unknown; a profitable close alone is not established profitability or an official contest result.

The prepared owned-risk V2 continuation distinguishes official positions from resting orders. Official exposure blocks new paper buys, while causal, fresh, complete account observations with no own resting order permit counterfactual reduce-only valuation/exits. Missing/stale/future observations and our open orders continue to invalidate marks and block fills. Protected collector obligations remain intact; verified held-risk hints replace existing optional read slots. The serialized 20-read budget and disabled contest writer remain unchanged. Account impact and queue priority remain unmodeled.

The continuation exports the original journal using recorded account projections and consumer clocks, checks its consumed frontier against the sealed source, and carries exact cash, FIFO lots and fills. It cancels boundary intents and requires future independent quotes. The original bank and journal are preserved, and the new opening state, bank, journal and frozen dependency graph are independently hash bound. Research expiry remains unchanged; it cannot authorize new entries after expiry. Deployment and live-verification receipts distinguish prepared code from the active wallet.

October 4 owned-risk continuation is active as source-probability-owned-risk-20261004-v2. At 10:24 UTC the collector, paper worker and rebuilt Next.js dashboard were healthy. Exact V1 bank/journal bytes and all cash, FIFO lots and fills were preserved. A verified held-risk read replaces one optional slot, with zero additional requests. The current frozen dependency closure contains 49 files and imports successfully. Thirty targeted checks and the production dashboard build pass; the legacy VM runner harness fails before any requests at a missing performance binding even without this change, so the entire repository suite is not claimed green.

The latest fresh paper quote gave whole-wallet liquidation P&L of -0.010, with +0.020 realized from one completed trade and one share still held. Both latency cases share the same trade. The dashboard expires marks after 20 seconds; expired quotes become unknown rather than current profits. The source-probability research expiry was not extended. No contest orders, read slots or persistent services were added. The slow Node paper-host startup remains a separate optimization target from the measured native replay speedup. See data/probability-owned-risk-live-20261004.json.

October 4 native source transport is active as source-probability-native-20261004-v3. Native public quote validation preserves the original chain, scope, clock, depth, session, invalidation and source-version checks. Exact full-state parity on 15,879 real records took 1.50 seconds in Rust versus 48.94 seconds in original JavaScript (32.66x). The deployed paper worker cold start took 1.82 seconds. New rows still use the frozen incremental JavaScript probability/accounting reducer. The native binary, runtime library and 14-file compiled source graph are pinned and archived, and the new frozen paper closure has 54 files.

The V2 checkpoint was replayed with its recorded account projections and receipt clocks, verified against the published final state, and carried to the new opening state without resetting cash, basis or fills. V1/V2 original bank/journal bytes remain preserved. Boundary intents are canceled and future independent quotes are required. Thirty-eight targeted Node checks, two Rust checks and the production Next.js build passed; no full-repository test claim. The three services remain one collector, one probability worker and one dashboard, with the existing 20-read budget, zero fees and no contest writes. At 11:01 UTC the latest fresh liquidation P&L remained -0.010, with +0.020 realized, three fills, one close and one share held. Performance is counterfactual paper evidence; profitability and contest readiness remain unestablished. See data/probability-native-live-20261004.json.

October 4, 11:43 UTC: maker V5 and paired V3 are active with fresh NH rule attestation from issue comment 5978588243. Independently captured public settlement/rules/replacement sources retain exact hashes. The attested ballot remains peer evidence, not an independently inspected image. The earliest six-hour source bound expires at 15:36:07 UTC. Fusion, administrative zero payout and asymmetric cancellation remain explicit stresses. A cancellation refunds modeled held purchase principal, not a one-dollar payoff; actual platform refundable-cost allocation remains unknown. No guaranteed arbitrage or execution authority follows from this review.

The live maker V3 journal exposed a reservation deadlock: each post-expiry book advanced the quote's required trade-report checkpoint, over 500 times per old quote. V5 pins that checkpoint once and retains independent pagination chains for normal and historical coverage windows. A bounded recovery replaces existing auxiliary tape requests; it adds no API client or admissions. The original quote intervals and all cash, FIFO, fills and histories are retained. A quote canceled before its activation can release its reservation without claiming a fill.

Two complete official historical tape responses covered the old quote lifetimes at checkpoints above the fixed requirements. All four old quotes resolved without qualifying trades by 11:39:47 UTC. Each wallet released 0.945 reservation, with no change to cash, fills or P&L. New future quotes subsequently became eligible. At 11:43 the join wallet still had zero fills; the improved wallet retained its original -0.015 realized loss, and the paired wallet still had zero fills. This repair establishes accounting progress, not profitability. Forty frozen source files, predecessor byte/hash preservation, consumed source frontiers and 42 focused checks were verified. Three persistent services and the 20-read allocation remain unchanged. See data/nh-paper-v5-live-20261004.json and data/nh-paper-v5-preservation-20261004.json.

October 4, 11:59 UTC: the single collector now restores its public quote history through the pinned Rust validator. Versioned producer V3 emits byte-identical later book, invalidation and session records and immutable states compared with optimized JavaScript V2. Damaged original history, source/library changes, overlapping native opens, STOP during restoration and durable append failures reject startup. The original frozen wallet code remains intact, and all active maker/pair bank bytes and predecessor journal prefixes were checked after restart. Eleven native/publisher integration checks passed with no skips.

On the same sealed 34,157,554-byte / 16,301-record public prefix, optimized JavaScript V2 validation took 5.16 seconds and the native adapter took 1.54 seconds, a 3.34x improvement with exact complete-state equality. This differs from the earlier 32.66x comparison against the older original paper validator. Live producer startup measured 1.68 seconds; its prior JavaScript producer measured 5.44 seconds on a slightly smaller prefix. These are publication-stage measurements, not total collector startup or faster platform request pacing. At this milestone the separate approximately 4 GB collector journal remained JavaScript; its subsequent migration is recorded below. Incremental producer validation, transport and paper economics remain JavaScript. See data/native-collector-benchmark-20261004.json, data/native-collector-preservation-20261004.json and data/native-collector-live-20261004.json.

October 4, 12:45 UTC: the original collector journal now restores through a separate pinned Rust engine. It verifies canonical JSON.stringify semantics, complete record hashes and chain, independently binds the full source prefix, and returns the exact final body through the original Journal append interface. STOP remains responsive during restoration, and changed source/binary/library/compiled-source identities fail startup. The first preservation check wrongly assumed all paper journals used an empty genesis predecessor; the probability journal starts from its frozen bank hash. The check was corrected and full parity repeated before restart, without changing any wallet.

The same sealed 4.25 GB / 9,491-record / 2.31-day capture restored in 18.68 seconds including independent prefix hashing, versus 30.06 seconds JavaScript (1.61x). Earlier 1.01x and 1.20x prototype receipts are retained. On the full current 4.84 GB / 10,231-record live source, the final body, sequence and tail matched JavaScript exactly; live native recovery measured 21.94 seconds. All active probability, maker and pair bank bytes and journal prefixes were verified after restart. Sixteen targeted Node checks and two Rust checks passed with no skips. There are still three persistent services, the same serialized 20-read allocation, and no accounting reset or contest writes. Model replay's separate 10.46x acceleration and recovery speed do not establish profitability. See data/journal-native-benchmark-20261004.json, data/journal-native-preservation-20261004.json and data/journal-native-live-20261004.json.

October 4, 13:29 UTC: paired host V4/core V3 fixes a confirmed entry starvation defect. Exact replay of the original V3 decision journal found 126 fresh, positively admitted pair observations with eligible acquisition cost and zero entries. Each serialized first-leg refresh temporarily removed the old counterpart under causal account/freshness rules. The engine then cleared its non-executable confirmation marker and recreated it on the second leg, never confirming both later versions.

The correction retains that marker for at most the existing 120-second intent lifetime during a valid incoming refresh. Both legs must still have later independent requests/versions, current complete cofresh depth, positive reviewed capacity, fixed price caps and unchanged cash/depth/latency/risk constraints. Explicit exclusions, unavailable account, expired rules, boundaries, marker expiry or a complete ineligible pair clear it. Twenty-three focused checks pass, including actual serialized cycle spacing, hedge/orphan losses, restart accounting and declared frozen dependencies beyond static imports. The latter check covers a preparation failure: the initial new archive omitted an unused dependency still pinned by the bank; the complete 36-file archive was restored before starting, without changing the new bank/seed or any original wallet.

Historical replay of the same decisions produces one counterfactual completed pair under the correction, with zero realized P&L, -0.035 liquidation P&L and +0.015 ordinary conditional settlement surplus. No parameter selection or profit claim follows. The new continuation `baal-no-pair-anchor-20261004-v4` subsequently made two hypothetical buys from independently later captured requests, completing one pair with cost 0.990. Through 13:28:58 it had zero realized P&L, -0.030 liquidation P&L, and +0.010 conditional ordinary settlement surplus; fusion/admin zero payout could lose the full 0.990. Original probability/maker/pair banks, journal prefixes and cash histories were preserved; native collector recovery, three services, zero fees and the 20-read allocation remain in place. The local dashboard displays the corrected pair's two hypothetical trades and two positions. This is a working paper entry path, not proven profitability or contest execution readiness. See data/pair-anchor-live-20261004.json and data/pair-anchor-milestone-20261004.json.

October 4, 14:04 UTC: the native wallet now supports the explicit
`fillClockPolicy: "independent_request_latency_v2"`. It requires the later book's
request to start at least the configured latency after the intent decision;
response time alone is insufficient. Liquidation freshness uses request age.
Every accepted V2 fill includes its decision/request/receipt clocks and exact
version lineage. Nanosecond timestamps and sequence IDs are strings to preserve
values above JavaScript's safe integer range. The absent/default
`receipt_latency_v1` policy remains solely for reproduction of frozen history.
No active live wallet, source journal or collector binary was replaced.

V2 evaluator declarations use `causal_dip_subsequent_v2` together with that
strict clock. Trading starts after `declaredAt`, including independently started
requests; all earlier books can only warm features. Missing post-declaration
data and mismatched clock/declaration versions reject. For development reuse,
`causal_dip_development_diagnostic_v2` explicitly retains a diagnostic label.
Build with `./backend-rust/build.ps1 -Test` and run the existing
`npm run evaluate:rust` adapter against an immutable declaration and source
prefix. Declarations/results remain distinct from the running paper wallets.

Twenty-one native Node checks and two Rust checks passed, with no skips. All 180
legacy cards match the original historical comparison exactly. On that same
2.31-day prefix the stricter clock verified 1,252 modeled fill clocks and changed
zero wallet outcomes: the older observed snapshots already exceeded latency.
The regression tests demonstrate the latent defect; it is not an explanation for
the measured historical losses. The original numeric-clock receipt is retained
as `data/native-request-clock-milestone-20261004-v1.json` before the lossless
encoding correction.

The unchanged 100 strategy/cost cases were frozen at 13:47:13.407 UTC. The full
4.98 GB / 10,398-record / 2.61-day source processed in 13.25 seconds, including
13.07 seconds of original-byte verification and 111 ms of parallel native
comparison. Only the 35 records / 28 cycles after the freeze can contribute
wallet activity. Through 14:03:40.666 UTC, 72 variants made a single identical
paper entry; the other 28 stayed flat. This is one correlated source event on one
instrument, not 72 independent trades or additive profit. Held variants have
99.125 cash, one share, zero realized P&L and no completed trade. Their last
liquidation marks are unavailable; the -0.875 cash-only inventory-zero floor is
not a realized loss. Fees are zero, and the sample is too short to establish an
edge. Three services and the serialized 20-read account allocation remain intact.
See `data/native-request-clock-declaration-20261004.json` and
`data/native-request-clock-milestone-20261004.json`; raw fills stay in restricted
runtime storage. No contest orders or additional persistent process were added.

The frozen 100-case Rust cohort now refreshes inside the single collector through
`native-dip-host-v1.mjs`. Every three minutes it seals an original journal prefix,
checks append continuity, verifies the pinned binary/declaration, and evaluates
the unchanged wallets. Separate exact cash/FIFO/clock checks derive held-market
read obligations. Those obligations use the existing four-book scheduler and
remain protected when a replay fails; only a verified close releases them. A
bounded, abortable native child adds no API client or persistent service. Frozen
pair, maker and probability banks and their original journals are preserved.
Ninety-six focused accounting, scheduling, host and actual runner checks pass.

The first unique subsequent dip round trip closed before this integration was
deployed. It lost 0.010 per participating wallet: +0.005 midpoint movement minus
0.005 quoted spread/ladder cost and 0.010 two-sided slippage, with zero fees.
Seventy-two copied results represent one correlated trade, not 72 independent
experiments. The remaining 28 cases stayed flat. The initial automatic update
verified 5.05 GB / 2.64 days and compared 100 cases in about 23 seconds including
the separate append-continuity hash; native comparison itself took 124 ms.
This is an engineering milestone, not profitability evidence. Pending intents
still lack per-instrument risk hints, and shared read capacity can still limit
freshness. API ingestion and Next remain Node; capture verification and wallet
comparisons run in Rust. See `data/native-dip-host-milestone-20261004.json`.

The versioned V2 native host uses a pinned compact capture cache and verifies the
canonical append. The original source bytes are still fully rehashed on every
use, including the immutable previous result's digest/boundary. The same 5.086 GB
prefix takes 4.25 seconds instead of 14.02 seconds in native evaluation (3.30x).
The cache is 27.24 MB instead of 5.048 GB (185x smaller). All 100 scorecards match
both full native evaluation and the original frozen engine; models, declaration,
fees and prior wallet accounting are preserved. V2 removes the separate Node
source-hash pass and refreshes after collection cycles. The first prepared live
continuation took 4.58 seconds, and the first deployed update took 4.90 seconds.
There are still three services and the same coordinated read allocation, with
zero contest writes. 132 Node checks plus two Rust checks pass, with no skips.
The dip trade remains a 0.010 loss per copied participating wallet; these speed
measurements do not establish profitability. Useful full-book discovery and
per-instrument pending-intent monitoring remain the next strategy bottlenecks.
See `data/native-cache-milestone-20261004.json`.

Native collection now uses a collector-local, verified minute-history screen
instead of the retired history worker. It bootstraps the original public quote
chain, anchors prior checkpoints, and consumes only durably published receipts.
The six-hour minute window, 30-minute evidence floor, causal current/prior-minute
exclusion, and two-cent prior-bid-minus-current-ask screen are unchanged. Prefix
pruning produces the exact same state as the unchanged minute-model oracle.
The same 103.71 MB / 9,258-receipt capture rebuilds in 2.48 seconds instead of
34.49 seconds (13.89x), with identical counts and leads. This was a screen-speed
comparison, not a backtest of profits.

Native scheduling removes the old fixed-core comparison reservation and retains
actual held/pending obligations first, including verified reduce-only paper
monitoring of an owned exposure. The remaining slot requests fresh depth for a
screened candidate; every fourth actual discovery attempt retains persisted fair
coverage. Failed attempts advance the durable ordinal, while overwritten or
unattempted plans do not. The same four-book and serialized 20-read budgets apply.
Bulk quotes remain non-executable, account exclusions still prevent entries, and
screen failure suppresses leads while retaining risk collection. Original
buy-hold benchmark marks may be stale because that historical passive benchmark
has no exit strategy. No frozen bank, declaration or wallet history was reset.
The paired-market review expired at 15:36:07.104 UTC on October 4; its held
positions remain monitored without renewed entry authority. 111 relevant checks
pass. See `data/native-discovery-benchmark-20261004.json` and
`data/native-discovery-milestone-20261004.json` for measured deployment evidence.

The V3 native continuation exposes pending intent instruments and their original
decision versions, limits, quantities, latency floor and expiry. A separate
verifier reconciles instrument scope, action/quantity against reconstructed
inventory, exact money, intent uniqueness, clocks and declared counts before any
read hint can enter scheduling. Expired entry confirmations stop requesting
reads; held inventory stays protected. Host failure retains verified risk. The
existing four-book budget still applies, with no additional API client or service.

The original trading engine source and declaration are unchanged. Evaluation
adds opt-in serialization through a const inspector; ordinary evaluation omits
it. On the same 5.224 GB prefix, all 100 original scorecards match the active
archived engine, and all 100 full/cached observed scorecards match each other.
Cached observation took 4.40 seconds. The original host, binary, cache and outputs
are retained. 171 Node checks and two Rust checks pass; no refit or wallet reset.
See `data/native-intent-preparation-20261004.json` and
`data/native-intent-milestone-20261004.json`.

Current scarcity is measurable: individual frozen cases have only zero or one
edge-qualified signal; most observations fail deviation or cost-adjusted-margin
gates. There are no pending intents in the measured current cohort, so adding
confirmation hints alone cannot establish profit or fix feature coverage.
The next strategy comparison needs causal broad quote-history features, fresh
depth for fills, a new future cutoff and independent wallet accounting, while
retaining this sparse-depth cohort as an unchanged comparison.

The separate `baal-quote-history-20261004-v1` cohort is now running inside the
collector: 237 markets, 24 fixed quote-history strategies and 72 independent
latency/cost wallets. Its future cutoff was declared at
2026-10-04T16:54:44.307Z. Quotes warm causal, minute-weighted historical references;
only validated public depth on a later independent request can supply paper fills.
Account eligibility is recorded before books in a small private sanitized
journal. Missing historical eligibility never becomes a retroactive purchase.
Zero fees; depth participation, slippage, reserve, exposure, version and source
boundaries remain enforced. The original engine and all 100 prior scorecards
were exactly preserved. Three services and the existing four-book/20-read budget
remain; Daenerys retains competition execution and Codex's writer is disabled.

Measured 2.20 captured days / 146.37 MB replay in 4.13 seconds; feature/wallet
simulation was 0.49 seconds of that. This is a compact-source workload, not an
identical-strategy speed comparison. The live check through
2026-10-04T17:01:38.917Z recorded no fills or gains yet, but a pending market311
entry entered the bounded confirmation read plan. The old sparse-depth cohort
still has one unique losing roundtrip of -0.010 per participating copied wallet.
Profitability is unestablished. 104 Node checks and three Rust checks pass.
See `data/native-quote-preparation-20261004.json`,
`data/native-quote-live-20261004.json` and
`data/native-quote-milestone-20261004.json`. Continue subsequent confirmations and
measured execution/cost diagnostics; signals, test fixtures and correlated
wallet counts are not profit evidence.
