# Current Moloch and Tartarus state

Observed October21,18:03UTC. This page replaces repeated historical headlines; [the previous document is preserved in full](CURRENT_STATE_HISTORY_20261021.md).

## Production

R67 PID1014772 recovered at17:11:58 with healthy realtime data and the original420 fills preserved. It then produced five new fills on order54128846/native20213:5,110 NO shares of exchange851 sold at0.895. Exact official receipts and FIFO recomputation establish4,573.45 proceeds,4,460.24 released cost and113.21 realized gross. Remaining433 shares of that original5,543-share order were not terminal at the sale audit.

At17:19:05.600 R67 stopped after repeated stream queue overflow. R68 was installed at17:35:25 as sole PID1018590/startTicks57378000 with executable9fd14df514bd and controlab83474a6374 verified. Recovery is complete:17:56:30 and18:02:26 progress prove current-process trading with healthy realtime data. All425 previous fills and the original2.545GB financial prefix are preserved. Both older orders20213/20215 are terminal. At18:02:26 there are430 fills,1671 acknowledgments and1668 terminal orders: three new R68 acknowledgments, no confirmed new R68 fills. [Fresh verification](../research/tartarus-learning-v2/r68-fresh-live-20261021.json).

R68 also reconciled R67 order54136675/native20215, executed17:18:54 before the old shutdown:6,521 exchange872 YES shares sold across five fills at0.41–0.42. Exact original receipts and FIFO verify2,721.82 proceeds against2,999.66 released cost, a277.84 realized loss. It is not a new R68 execution. The two audited R67 sales therefore net−164.63. [Loss and original attribution](../research/tartarus-learning-v2/r67-second-sale-reconciled-20261021.json).

The repair increases native ingress from64 to1024 messages with a64MiB encoded-payload budget and preserves FIFO. Recovery retains the existing rolling attempt/token/account budgets and waits for the next admission instead of permanently stopping when the reconnect window is full. Protocol errors, unknown order acceptance and durable corruption retain their existing handling. Financial-engine identity and accounting are unchanged. All289 source files were compared against the deployed R67 source; only the six intended transport paths differ.30 realtime,12 connect-retry and8 token-retry checks pass. [Build, source pins and actual installation](../research/tartarus-learning-v2/r68-stream-throughput-installed-20261021.json).

Last reconciled native financial state:

| Measure | SUSQies |
|---|---:|
| Cash | 16,896.10 |
| Remaining attributed position cost | 78,849.72 |
| Total attributed realized gross | 165.905 |
| New-trading portion | 89.385 |
| Unchanged inherited realized portion | 76.52 |

Cost is not current market value. These figures do not establish whole-account profit or achievement of the missed97,000 deadline. [Sale audit](../research/tartarus-learning-v2/r67-first-sale-20261021.json), [R67 fresh-recovery proof](../research/tartarus-learning-v2/r67-fresh-live-20261021.json).

There remains one official worker and shared limiter. Daily windows and original financial caps are unchanged. The separately built full-universe native capacity change is not deployed. Laptop training creates no account client.

A combined release candidate now includes the published full-universe change, immutable review-map sharing and a direct single-order lookup.150 focused checks pass; the lookup avoids rendering the entire20,000-order history on cache misses. A synthetic component benchmark drops the second full-render sample's144.47ms/lookup to roughly0.068ms; complete worker speed has not been measured. Full current-ledger equivalence and the release build remain in progress. R68 stays active during qualification. [Scope and exact limits](../research/tartarus-learning-v2/single-order-lookup-20261021.md).

The actual laptop external feeds are healthy:Kalshi237 tickers at1,193 REST admissions/minute, oldest receipt11.9s, no throttles; Polymarket378 outcome books/189 markets, configured30 REST admissions/s, receipt ages3.19s median/6.37s maximum. Both websocket feeds are receiving. A retired VPS Polymarket status file is not the live feed. These are observation freshness figures, not contract-equivalence or profit evidence. [Measured feed state](../research/tartarus-learning-v2/current-external-feeds-20261021.json).

## Learning

Active037 remains on its original frozen source. Its first island completed2,773 optimizer updates/36,730 decisions and entered full-window evaluation. Its second island completed one member, then resumed learning another. No independently profitable checkpoint has emerged.

The2,768-update checkpoint's complete October17–19 development results were-4.35% nominal/-4.94% stressed marked ROI. Its earlier founder is stronger by the human's daily-growth reward. The other island's first member produced only0.05 SUSQie nominal marked gain on100,000 initial wealth and zero stressed; this is near-inactivity, not a demonstrated edge.

Queued040 preserves the completed island populations before source migration. It includes founder retention, independent learned BUY/SELL target selection,64-transition PPO batches, faster equivalent history validation and owned-response delivery. The last optimization matched every observation, action, paid request/receipt and financial hash in four interleaved fitting-hour passes; measured collection speedup was1.040x alongside existing GPU training. That is not end-to-end training speed or ROI.

A separate optional process-evaluation implementation passed22 unique focused checks and exact parity. Four interleaved CPU passes over the same five-minute development slice took116.24s median locally versus154.12s with child worlds:32.6% slower including startup/IPC. It remains disabled and is not added to037/040. No optimizer step or protected-test access occurred. [Negative benchmark](../research/tartarus-learning-v2/process-evaluation-benchmark-20261021.json).

The full requested competition-history training plus4-day evolution/4-day independent test remains incomplete. Current detailed development uses2 fitting days and2 development days. ProtectedOctober21–25 is unconsumed; no learned model is deployed. [Implementation and evidence](../research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md).
