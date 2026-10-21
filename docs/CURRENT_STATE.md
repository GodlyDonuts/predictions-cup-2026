# Current Moloch and Tartarus state

Observed October21,17:29UTC. This page replaces repeated historical headlines; [the previous document is preserved in full](CURRENT_STATE_HISTORY_20261021.md).

## Production

R67 PID1014772 recovered at17:11:58 with healthy realtime data and the original420 fills preserved. It then produced five new fills on order54128846/native20213:5,110 NO shares of exchange851 sold at0.895. Exact official receipts and FIFO recomputation establish4,573.45 proceeds,4,460.24 released cost and113.21 realized gross. Remaining433 shares of that original5,543-share order were not terminal at the sale audit.

At17:19:05.600 the worker stopped on `STREAM_CONNECT_ATTEMPTS_EXHAUSTED`, after repeated `STREAM_QUEUE_FULL`. The last17:18:56 progress had425 fills,1668 acknowledgments and1666 terminal orders. Known nonterminal orders remain authoritative and must be reconciled by the continuing worker. The stream-throughput replacement is being qualified; it is not yet deployed.

The repair increases native ingress from64 to1024 messages with a64MiB encoded-payload budget and preserves FIFO. Recovery retains the existing rolling attempt/token/account budgets and waits for the next admission instead of permanently stopping when the reconnect window is full. Protocol errors, unknown order acceptance and durable corruption retain their existing handling. Financial-engine identity and accounting are unchanged.

Last reconciled native financial state:

| Measure | SUSQies |
|---|---:|
| Cash | 14,174.28 |
| Remaining attributed position cost | 81,849.38 |
| Total attributed realized gross | 443.745 |
| New-trading portion | 367.225 |
| Unchanged inherited realized portion | 76.52 |

Cost is not current market value. These figures do not establish whole-account profit or achievement of the missed97,000 deadline. [Sale audit](../research/tartarus-learning-v2/r67-first-sale-20261021.json), [R67 fresh-recovery proof](../research/tartarus-learning-v2/r67-fresh-live-20261021.json).

There remains one official worker and shared limiter. Daily windows and original financial caps are unchanged. The separately built full-universe native capacity change is not deployed. Laptop training creates no account client.

## Learning

Active037 remains on its original frozen source. Its first island completed2,773 optimizer updates/36,730 decisions and entered full-window evaluation. Its second island completed one member, then resumed learning another. No independently profitable checkpoint has emerged.

The2,768-update checkpoint's complete October17–19 development results were-4.35% nominal/-4.94% stressed marked ROI. Its earlier founder is stronger by the human's daily-growth reward. The other island's first member produced only0.05 SUSQie nominal marked gain on100,000 initial wealth and zero stressed; this is near-inactivity, not a demonstrated edge.

Queued040 preserves the completed island populations before source migration. It includes founder retention, independent learned BUY/SELL target selection,64-transition PPO batches, faster equivalent history validation and owned-response delivery. The last optimization matched every observation, action, paid request/receipt and financial hash in four interleaved fitting-hour passes; measured collection speedup was1.040x alongside existing GPU training. That is not end-to-end training speed or ROI.

The full requested competition-history training plus4-day evolution/4-day independent test remains incomplete. Current detailed development uses2 fitting days and2 development days. ProtectedOctober21–25 is unconsumed; no learned model is deployed. [Implementation and evidence](../research/tartarus-learning-v2/IMPLEMENTATION_STATUS.md).
