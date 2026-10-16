# Paper 6: cross-venue state, payoff floors and tournament capital

Received October16, 2026. Original private attachment SHA256
`be96a03a94886ac984d31f6d517a9cfee6de6054f2e3ed5d957c19dd227e136b`.
Task [#155](https://github.com/GodlyDonuts/predictions-cup-2026/issues/155).
The human makes the entire available SUSQie balance eligible for Moloch.
That expands capital authority; it does not turn additional deposits or borrowed
collateral into trading profit. Original financial history remains attributed.
Embedded citations and historical balances in the paper are research leads,
separate from current observations and the human's direct authorization.

## Latest operation and qualification at17:39UTC

Sole PID **525450**, source `b112c6c1` / binary `3ab6ef7c`, is verified **fresh trading** at17:39:29UTC: healthy realtime, matching executable, no STOP, thirteen post-launch official order acknowledgments and two reconciled fill rows. `defensiveCancel:true` is active. Incremental progress and the completed full-cycle status now both match525450.
[Actual fresh verification](../research/moloch-book-freshness-semantics-20261016/production/full-cycle-verification.json).

The two fill rows sell1188 residual YES shares at0.340 against0.335 original basis, adding **5.940000 SUSQies** realized after zero fees. These are exits of inventory acquired before this restart, not proof of new-entry alpha. Cumulative fills are200; ordinary realized **−176.595000**, inherited gross **+45.130000**, combined **−131.465000**. Cash72,716.935000 and owned cost22,731.515000 are the attributed bank view. Contributions55,579.915000 remain separate from profit within the full95,579.915000 capital bound. Overall profitability remains unestablished.

The same original30s metadata-only probe observed869 books/26 frames/219 exchanges. Median receipt-to-probe lag is **838ms**, versus2069ms in the prior window; p90 is6064ms versus7923ms. Median original source age is1747ms versus4157ms. The windows differ, share frames and lack an append clock: this does not establish a causal whole-cycle speedup or profitable strategy.
[Original probe and fixed reproduction](../research/moloch-defensive-cancel-20261016/measurement/README.md).

Actual ARM64 SDK231/runtime-library197/worker95 checks and optimized release passed. Production services the existing stream during ordinary/reviewed preflight reads and can cancel source-bound resting ordinary BUYs that fail the unchanged entry criterion. Native financial engine8acc, the original bank, twelve bridges, existing account budget and daily windows remain. No additional account client was launched.
[Checked build](../research/moloch-defensive-cancel-20261016/build/README.md) | [Design](../research/moloch-defensive-cancel-20261016/worker-design.md).

Read-only trace V5 PID526213/source2fa9 started17:38:10 against525450's genuine journal, finite3600s, after normally sealing V4; SIG was not restarted. The genuine `(sequence, original read timestamp)` observer repair passes103 checks. Its fixed versioned replay passes24, the adapter14 and parent analysis22 locally and on Linux; original V2 result bytes reproduce exactly with the historical sampler. Original V3 replay verifies ten emitted rows/seven labels. Combined V2+V3 analysis was not run because at most twelve rows cannot meet the frozen minimum; no fitted cross-venue model is qualified. All three probes observed zero same-sequence/newer-time cases, so no recovered-sample gain is claimed.
[Book-version semantics and actual deployment](../research/moloch-book-freshness-semantics-20261016/production/README.md).

Kalshi's existing collector retains237 tickers,20 reads/sec target and WebSocket reception. The remaining3 candidate-specific analogs were audited: **zero new payout-equivalent mappings**. Candidate-win and party-win contracts remain distinct; live mapping did not change.
[Resolution evidence](../research/moloch-remaining-mappings-20261016/README.md).

## Historical operation and qualification at 14:57 UTC

Actual516085/source870ce889/d2ad93b5 is fresh trading, healthy realtime/no STOP:
eleven new ACKs at14:57:24UTC,173 cumulative fills/−106.485000 realized.
Explicit side enforcement has actual below-minimum deferral and meets-minimum
continuation; the transition fill round is flat and includes an old resting entry.
All278 distinct ARM64 checks/release passed. Same bank, native engine/twelve
bridges, capital and daily windows remain. [Live evidence](../research/moloch-ordinary-side-edge-20261016/README.md).

The larger frozen v9 price-only comparison still selects zero after-cost trades
despite22,579 decisions and adequate OOS counts. No qualified cross-venue model
is inferred. [Actual result](../research/moloch-crossvenue-economics-20261016/v9-price/README.md).
A separate trade-triggered sampling mode is checked78 Python/staged; it requires
original trade clocks and a current retained SIG book. Default rows/old cohorts
remain intact. [Separate prospective protocol](../research/moloch-trade-trigger-20261016/README.md).

Sealed v7 flow evidence was correct:0/5/48 identified1/5/30s inputs exactly
match receipt-eligible original same-ticker trades. No lost exported trade or
producer bug was proved; optimistic instant delivery yields at most3/8/50,
insufficient for the frozen model's threshold. Five checks and full original
capture reproduction passed. [Coverage diagnosis](../research/moloch-crossvenue-economics-20261016/v7-coverage-audit/README.md).

## Historical operation and qualification at 14:35 UTC

Current sole510591 is fresh trading,40 new ACKs/eight new fill rows at14:33:40;
realized−106.485000. Checked source870ce889/released2ad93b5 is staged after278
distinct passing ARM64 checks and a successful optimized build. Explicit
ordinary-side enforcement screens independent BUYs with the original minimum
side criterion; covered hedges/exits and existing limits remain. Separately
typed token-HTTP retry uses the existing durable bounded attempt budget.
Neither change is yet in the running worker. [Actual build and selected mode](../research/moloch-ordinary-side-edge-20261016/README.md).

Measured realtime processing median12.524s→2.960s/p9024.467s→12.304s is a
bounded observational result, with no CPU/whole-cycle/profit claim.
[Actual latency, unique-fill accounting and limitations](../research/moloch-inter-read-20261016/README.md).

## Historical operation and qualification at14:04UTC

Recovered510591/source4ee/executable011ffde0 is genuinely fresh trading at
14:03:28UTC with healthy realtime/no STOP, one official new ACK; five by14:04:45,
zero new fill rows. Original163fills/−106.485000 realized/full 95579.915000 capital
remain. Read-only674 observer510713 starts new v9; old v8 stays sealed.
[Actual recovery/readiness](../research/moloch-inter-read-20261016/README.md).
The fixed actual-fill study exposes weak one-sided entries: two covered original
submission contexts fail the existing0.001 microprice criterion and account for
−24.34 displayed marks, yet retained fills also lose. This supports a prospective
entry-selection experiment, not a profit claim. Optional runtime wiring and
separate bounded token HTTP recovery are being checked while production runs.
[Actual study and limitations](../research/moloch-maker-markout-20261016/README.md).

## Historical operation and qualification at13:54UTC

Initial successor506740 stopped13:47:15 with CONNECTION_UNCONFIRMED at token
HTTP after restoring original history. No stream/financial submissions began.
Root retained failure evidence, verified actual SDK quiet/zero unknown and
unchanged financial journals/bridges, then manually started the same binary
once as510591 at13:51:29. At13:54:14 it was restoring; fresh trading is pending.
Daily authorization/token budget and financial history were not reset.
[Actual repair evidence](../research/moloch-inter-read-20261016/README.md).

## Historical operation and qualification at13:43UTC

Inter-read runtime4ee16356/executable011ffde0 passed81 worker checks and an
optimized ARM64 build, then started as sole SIG506740 at13:36:03UTC. At13:43:24
it was still restoring original history: fresh trading is pending. All native
bytes, original bank and twelve bridges remain unchanged. No additional bridge,
detached-copy replay or principal recredit; only activation.binarySha256 changed
in control. The new hook drains the existing source before ordinary reconcile
reads; no drain interrupts immediate receipt validation/fill application.
[Actual promotion and measurement boundary](../research/moloch-inter-read-20261016/README.md).
Stopped totals:163 fill rows, ordinary realized−151.615000, inherited gross45.130000,
combined−106.485000; principal95,579.915000 remains separate from profit.
The offline prize adapter's additional account-origin/FIFO checks passed13
focused ARM64 tests at13:39:41. These are published source changes, not a
qualified whole-account opponent model or physical prize-policy integration.
Kalshi continues collecting; observer v8 is sealed before successor rebinding.

## Historical operation and qualification at13:15UTC

The buffered writer is now deployed on sole SIG503120/source31aefba5, with
fresh trading/realtime and26 new official ACKs at13:14:49UTC; seven post-launch
fill rows. Attributed realized−81.215000; fills are not a profitability claim.
Full95579.915000 allocation, original funding, daily windows and
liabilities are preserved. Actual original-prefix/twelfth-bridge replay passed
455,108ms before promotion. The report-writing benchmark improved49.95times;
the live comparison found12.524s median realtime backlog versus11.383s before,
with no broad latency win. The first30-scope cycle completed13:10:59 in13m41.923s;
the baseline had35 scopes, so workload differences remain explicit. Inter-read
stream service is the next targeted runtime change; production continues.
[Actual deployment](../research/moloch-hotpath-20261016/deployment-README.md).

Observer503257 records a separate v8 cohort. Sealed v7 had3,883 decisions and
5,837 labels but insufficient identified flow to fit the frozen three-window
comparison. [Coverage evidence](../research/moloch-crossvenue-economics-20261016/v7-flow/README.md).
Actual ordinary capacity already allows12 fresh/40 total exchanges; no change
to historical seed3 is justified. [Capital audit](../research/moloch-capital-utilization-20261016/README.md).
Official netting is usable, but canonical same-market acquisition costs1+spread;
all-principal refunds prevent a strictly positive minimum terminal gain. No
profitable certified package was found. [Semantic audit](../research/moloch-semantic-floor-20261016/README.md).
Whole-portfolio tournament-prize scoring compiles with12 focused ARM64 checks.
It handles exact full-account cash/FIFO/debt, explicit conditional joint opponent
atoms, declared prizes/ties, stress cases and same-state do-nothing comparisons.
It is offline and cannot score the current larger portfolio from a six-contract
surface; candidate search and physical use still need genuine complete joint-state,
liability and leaderboard evidence.
[Scorer and actual checks](../research/moloch-prize-utility-20261016/README.md).

## Historical operation and qualification at12:40UTC

The checked buffered writer has built separately and passed301 distinct Rust
checks. On the exact original production report it reduced physical write
calls668,119to29 and wall time682.431ms to13.663ms, with exact byte parity
and file/directory syncs retained. Whole-loop speed remains to be measured.
[Actual benchmark](../research/moloch-hotpath-20261016/buffered-benchmark-README.md).

Original494293 stopped cooperatively12:36:21 for this narrow promotion; the
actual247,652-event stopped prefix is under detached-copy verification before
the genuine twelfth bridge and replacement start. Full funding, daily windows,
bank and outstanding liabilities are preserved; new trading is not yet claimed.

The fixed v6 ablation establishes almost no observed external-price variation:
all available1s/5s changes are zero. SIG-local ridge beats the full model at
four horizons, all models select zero trades, and no incremental Kalshi edge
is identified. This is a useful negative diagnosis, separate from the new
signed public-flow cohort and from complete Paper6 strategy qualification.
[Ablation](../research/moloch-crossvenue-economics-20261016/v6-ablation/README.md).

## Historical operation and qualification at12:22UTC

The full-capital worker494293 remains the actual deployment, with34 new ACKs
and one post-launch fill row at12:22:54UTC; attributed realized remains−74.405000.
The signed public-flow feed and source-bound observer are now live separately
without a SIG restart. A larger sealed cohort of1,348 decisions/1,903 labels
still selects zero after-cost trades; all944 complete toxicity states areQUIET.
No calibrated external-price or cancellation authority is inferred.

The new mechanical bottleneck is measured local I/O:3.75million tiny writes
in10seconds, with a19m21s first35-scope pass. The narrowly buffered writer is
checked source under separate optimized build; it preserves exact files and
durability. Deployment and a genuine subsequent latency comparison are still
pending. See [the timing audit](../research/moloch-hotpath-20261016/README.md),
[live public flow](../research/kalshi-signed-flow-20261016/README.md), and
[the larger fixed-model result](../research/moloch-crossvenue-economics-20261016/all-market-v6-README.md).

## Historical operation and qualification at 11:51 UTC

Full-account production is active on sole SIG494293/source`e7bb62d1` with its
matching immutable binary, healthy realtime and fresh per-scope trading. Seven
new official ACKs and one post-launch fill row were verified at11:51:16UTC.
The55,579.915000 owner contribution and explicit daily-mandate revision are
committed, giving effective initial/commitment95,579.915000. Attributed realized
remains−74.405000; new capital is not profit. The full stopped-prefix replay
matched the original financial state before the eleventh bridge was installed.
[Original deployment and actual observations](../research/moloch-full-account-20261016/README.md).

Bulk negative screening and bounded rotating all-market capture are deployed.
The updated read-only observer preserves source-specific innovations through
temporary REST fallback and writes causal lag-anchor evidence. At11:51:14UTC,
it had378 decisions/487 labels/188 markets. Existing sealed economic studies
remain unqualified for live alpha; more data alone is not proof of an edge.
Bounded restart conditions were installed without restarting the SIG process;
the separate legacy SDK crash-lease limitation remains documented.

The remaining Paper6 work is economic qualification and execution integration
of cross-venue information/toxicity, genuine semantic completeness certificates,
deployed tournament-prize utility, and genuine election-night data vintages.
Full capital availability does not establish completion of those capabilities.

## Historical operation and qualification at 10:43 UTC

The sole SIG worker441490/source`fec81a5e` remains live. At10:39:26UTC its
binary matched, STOP was absent and trading/realtime were fresh. Since08:55
restart it recorded79 acknowledgments and four fill rows,136 cumulative fill
rows. Cash17,194.180000, owned cost 22,731.515000 and combined attributed
realized−74.305000 were unchanged. Fill rows can be partial executions and
are not a count of independent profitable trades.

The integrated Kalshi WebSocket/REST collector now has a genuine live socket.
At10:29:52UTC its current connection had237 valid reconstructed books and
zero sequence gaps. REST measured20 starts in the last second and1193 in the
last minute; one recovered network failure was recorded. The collector retains
original clocks, rejects stale views and quarantines bounded future clocks.
See [dated live evidence](../research/kalshi-websocket-20261016/live-verification.json).

The first sealed prospective study contains200 decisions and291 future labels
across35 markets. Held-out cluster counts at1/3/10/30/120seconds were
36/32/56/29/0. **Zero trades cleared the model's cost threshold; every horizon
abstained.** Short-horizon prediction error improved against the intercept in
this small sample, but did not establish executable profit. This is a one-share
crossing counterfactual with a0.005/share buffer, not actual fills. The report
pins the original producer version rather than silently rewriting legacy IDs.
[Original report and limitations](../research/moloch-crossvenue-economics-20261016/README.md).

The full-account successor is built separately while production continues.
It adds55,579.915000 as owner capital, giving effective initial capital and
maximum commitment95,579.915000; the contribution is not profit. It also adds
bulk negative screening, explicit daily-mandate migration and bounded rotating
all-market capture. Independent review found a funding-capability timing bug
that could stop ordinary exits; a narrow runtime fix is being built before
deployment. **The40,000 production allocation remains unchanged at this
observation.** Source checks and build success are not live deployment.

## Historical operation at 09:43 UTC

SIG production PID441490 remains on immutable source `fec81a5e`, with the
matching binary, fresh trading/realtime and no execution stop. Since its08:55
restart,34 order acknowledgments and two actual fill rows were observed.
Ordinary realized−119.435 plus inherited realized gross45.130 gives combined
attributed realized−74.305. This is distinct from daily dashboard valuation,
and does not establish profitability.

The sole Kalshi REST collector PID34280 is supervised by PID18348. All237
references were active and cached at09:43:20UTC,20 request starts in the last
second, zero errors/throttles at that observation. The oldest cached receipt
was11.992seconds old at09:43:19. The20/sec allowance covers all initialization,
metadata, discovery and book GETs. Later transient errors remain observable in
status; a successful earlier check is not an uptime guarantee.

Coverage is234 direct party references and three candidate analogs, not237
certified equivalent contracts. See [the exact mappings and original metadata](../research/moloch-kalshi-mapping-20261016/README.md).
The read-only cache is reachable on the VPS through an existing loopback SSH
forward; the key stays on Windows. Kalshi-derived production pricing is not yet
activated. A Windows or SSH outage must make the external data stale, never
silently refresh its clocks.

## Complete loops being implemented

| Work | Present state | Economic and operational completion |
| --- | --- | --- |
| Authenticated Kalshi shadow stream |237-reference REST and original-clock WebSocket cache live; actual snapshots/deltas/tickers/trades verified | Feed availability is established at dated observations; pricing calibration and execution eligibility remain separate |
| Prospective SIG/Kalshi observations | Frozen v9 has22,579 decisions/35,457 labels and zero selected after-cost trades. Original V2/V3 input replays verify2/10 emitted rows, below frozen count minima. Prospective finite V5/source2fa9 is live17:38UTC | Keep original book/flow studies separate; investigate exact target-book coverage and evaluate a sealed event cohort under its predeclared analysis |
| Maker toxicity and informed execution | Explicit ordinary-side admission and optional defensive cancellation are deployed. Entry defer/continue is observed; optional defensive DELETE is not yet observed. Latest profitable residual exit adds5.940; overall ordinary realized remains−176.595. Source-innovation/router foundations are present | Depth microprice is not calibrated EV. Qualify an economic signal/joint fill-value model before claiming external pricing or avoided adverse-fill profit |
| Semantic guaranteed-floor search | Exhaustive-certificate verifier and solver implemented;13 ARM64 checks pass | No genuine venue completeness certificate exists yet; finite scenarios alone grant no physical trade authority |
| Full-account capital |55,579.915000 contribution and daily migration committed; effective95,579.915000 live | Exact original-prefix parity and fresh sole-worker sources verified; funding credited no profit |
| Tournament objective | Exact offline joint-state prize scorer/account-origin/FIFO adapter passes13 checks, included uninvoked in the runtime library; actual whole-account coverage/model/search unavailable | Include all existing holdings/liabilities once and genuine joint opponent/state assumptions; current compiled-state bounds and missing posterior prevent an actual prize-optimized portfolio claim |
| Genuine election-night vintages | Existing nowcaster; real first-available historical vintages still missing | Preserve corrections and publication clocks; never label final results as data available earlier |

The historical optional SIG breadth diagnostic stopped at128MiB around09:20UTC.
The deployed successor rotates128MiB independent chain segments within an8GiB
run bound, preserving frozen original bytes. The financial bank continues recording
current canonical books. The new observer reads the all-market capture without a new SIG client,
HTTP request, writer lock or mutation. It starts at the present end, verifies
the subsequent chain, and does not claim a full-history audit. Source and
receipt ages, local first availability, missing labels and mapping classes
remain explicit. One-share crossing/exit comparisons are counterfactual;
they are not actual fills or realized profit.

## Funding interpretation

The prior40,000 wallet allocation is not the whole official account. Expanding
it requires a contribution event that credits cash and effective initial
capital together, leaving realized P&L unchanged. A new daily monetary mandate
must explicitly bind the previous mandate and current human authorization;
changing a JSON cap cannot substitute for that event. Existing held inventory,
pending orders, financial leases and unknown acceptance remain reconciled.

Full capital availability does not require buying every opportunity. The
ordinary maker's existing negative results argue for directing the new capacity
toward measured cross-venue or certified structural opportunities. Capital
use, executable depth and strategy allocation are separate decisions.

## Sources and checks

Official Kalshi [WebSocket connection](https://docs.kalshi.com/websockets/websocket-connection)
and [order-book updates](https://docs.kalshi.com/websockets/orderbook-updates)
were checked October16. Snapshot plus signed delta messages are a different
data surface from independently polled REST books; keep their provenance apart.
The existing public SIG API capability audit and original parsed specification
remain in [Paper5 evidence](../research/moloch-paper5-20261016/capability-audit.json).

The deployed full-capital release has14 new reader/copy checks and four optimized
artifacts, with retained unchanged-source90 runtime/265 native financial/10 breadth
checks explicitly identified as reference evidence. The actual stopped-bank copy
also passed full financial parity. New signed-flow source and optional observer
inputs pass45 Node and67 Python checks; those source changes are separate from
the presently deployed feed/observer versions until their own live verification.
These checks verify selected integrity paths, not alpha. Production continues
while research and later successors are developed.
