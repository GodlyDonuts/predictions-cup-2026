# Paper 6: cross-venue state, payoff floors and tournament capital

Received October16, 2026. Original private attachment SHA256
`be96a03a94886ac984d31f6d517a9cfee6de6054f2e3ed5d957c19dd227e136b`.
Task [#155](https://github.com/GodlyDonuts/predictions-cup-2026/issues/155).
The human makes the entire available SUSQie balance eligible for Moloch.
That expands capital authority; it does not turn additional deposits or borrowed
collateral into trading profit. Original financial history remains attributed.
Embedded citations and historical balances in the paper are research leads,
separate from current observations and the human's direct authorization.

## Latest operation and qualification at 10:43 UTC

The sole SIG worker441490/source`fec81a5e` remains live. At10:39:26UTC its
binary matched, STOP was absent and trading/realtime were fresh. Since08:55
restart it recorded79 acknowledgments and four fill rows,136 cumulative fill
rows. Cash17,194.180000, owned cost22,731.515000 and combined attributed
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
| Prospective SIG/Kalshi observations | Read-only supervised observer live; first200 decisions/291 labels evaluated with purged walk-forward | All five horizons abstained, zero selected trades; collect dense all-market observations after rotating-capture deployment |
| Maker toxicity and informed execution | Existing source-innovation/router foundations; new paired data collection | Qualify an economic signal, then wire cancel/join/improve/bounded execution into the existing sole worker; no price-copying oracle or unmeasured directional confidence |
| Semantic guaranteed-floor search | Exhaustive-certificate verifier and solver implemented;13 ARM64 checks pass | No genuine venue completeness certificate exists yet; finite scenarios alone grant no physical trade authority |
| Full-account capital | Contribution, startup reconciliation and explicit daily migration implemented; final timing fix/build pending | Fresh sole-worker cash/positions/collateral/complete orders, exact original-prefix parity and controlled upgrade before actual contribution |
| Tournament objective | Existing joint-state/CVaR optimizer available; prize objective not yet deployed | Include original existing holdings and liabilities once, current official leaderboard observation and robust threshold distributions; expected value and prize utility remain distinct |
| Genuine election-night vintages | Existing nowcaster; real first-available historical vintages still missing | Preserve corrections and publication clocks; never label final results as data available earlier |

The deployed optional SIG breadth diagnostic stops at128MiB; its current file reached
that limit around09:20UTC. The successor rotates128MiB independent chain segments
within an8GiB run bound, preserving frozen original bytes. The financial bank continues recording current
canonical books. The new observer reads that journal without a new SIG client,
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

Current focused checks: Kalshi client/collector/universe/supervisor48 passed,
WebSocket21 passed, prospective observer14 passed and economic evaluation15
passed. The full-capital precursor passed265 native,86 runtime and10 breadth
checks and three optimized builds; the subsequent narrow runtime timing fix
requires its own checks/build. These verify selected integrity paths, not alpha.
The combined advanced SIG successor is under separate ARM64 validation and
remains distinct from the deployed `fec81a5e` binary. Production stays running
while successors are built and reconciled.
