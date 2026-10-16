# Reviewed forecast positions in the existing Moloch worker

Current specification, checked against deployed source4148fe5d and the October16 04:16UTC production observation. Read the [research baseline](MOLOCH_RESEARCH_BASELINE.md) for the current binary/settings and [CURRENT_STATE](CURRENT_STATE.md) for subsequent dated observations. Paper2/3 remain incomplete; this is the implemented forecast path.

## Independent entries and execution modes

Forecast opportunities originate independently of ordinary spread-maker proposals. The worker nominates the exact reviewed exchange, obtains original official facts and reserves a real native BUY through `reviewed_fundamental_entry_reserved_v1`. Full funding, actual cash/reservations, total/per-market caps, fresh-market slots, reviewed principal/quantity, price ticks, acquisition depth and write pacing apply. Required reads alone do not create an order.

Omitted execution policy retains the original join-bid behavior. An explicitly reviewed policy can prefer `joinBid`, `improveOneTick` or `boundedLimitMayRest`. The bounded mode may cross the ask within the reviewed ceiling; unfilled quantity may rest. It has no ordinary-order IOC, post-only or atomic-fill guarantee. Its native reservation freezes the model/source/review hashes, action, quantity and limit. Only the corresponding opaque physical decision permits the bounded BUY; ordinary maker BUYs retain their passive guard.

After native activation, the worker obtains a fresh final seven-receipt packet. It preserves the original qid, quantity and price; a changed/retired quote defers rather than reprices under the old binding. The exact physical request is durably bound before its original attempt. A smaller planned physical slice is fixed before send and must match the actual direct POST or recovered GET acknowledgment.

## Sources and value assumptions

An immutable review pins tournament, market, exchange, contract, side, complete original contract-node root, retained source bytes/hashes and availability clocks. The execution owner supplies ordinary payout probability, exceptional-event allowance, minimum exceptional payout, margin, principal/quantity limits and a fixed holding deadline. These assumptions are neither calibrated confidence bounds nor guarantee certificates.

The reviewed entry ceiling, before venue tick flooring, is:

`((1 - exceptionalAllowance) * ordinaryProbability + exceptionalAllowance * minimumExceptionalPayout) - margin`.

A new entry needs a flat target exchange, current approved review/sources, no unresolved physical submission and an exact funded native opportunity. Existing inherited inventory has a distinct origin; it cannot be relabeled as a new forecast purchase. Correlated companion contracts do not create a guaranteed offset. Acquisition depth and later executable liquidation capacity are separate.

## Holding, exits and inherited positions

Only actual attributed fills create a forecast holding. Ordinary opposite BUYs and the one-minute age liquidation do not manage that reviewed holding. A current value exit needs adequate fresh bid depth and a SELL price at least equal to reviewed expected payout before the entry margin. A better displayed bid cannot authorize an old lower limit.

The original fixed deadline or explicit current revocation permits a covered risk exit. Stale sources prevent new entries and value-driven exits, cancel known resting entries and preserve the original deadline; stale information is not a new sell forecast. The entry's original terminal GET and complete fill tape must reconcile before a subsequent exit. A fixed pre-settlement holding deadline can require an exit below settlement-model value: conditional payout surplus is not an executable return at that deadline.

Inherited management requires a separate original adoption event/receipt, exact original position row and remaining FIFO lots, financial head/fill prefix, quiet original account queue and an explicit pinned inherited-origin review. Native `reviewed_inventory_exit_reserved_v1` reserves genuinely covered inventory at current depth. Historical cost determines attribution, not a compulsory break-even exit. Existing Massachusetts7061YES is reviewed for management only, with no additional BUY authority.

## Cohort lifecycle and restart

Original complete official terminal/zero-fill receipts can archive a closed-zero entry and permit a genuinely new logical cohort. The actual Delaware q3085/order41655759 acknowledgment02:13:59UTC and cancellation02:22:05UTC had zero fills; it was archived03:32:17UTC. That key was never retried.

A bound entry that expired without any original attempt can be abandoned only with positive original not-sent, zero-fill/holding and no-physical-liability evidence, exact native retirement and matching durable sidecar transitions. A timeout or snapshot absence does not establish this. Closed partial/filled entries do not use zero-fill rearm. A tagged attempted entry that later receives an SDK no-admission proof remains exclusive until its separate sidecar lifecycle is resolved; the ordinary q3411 incident was unmanaged.

Startup reconstructs from original sealed dispatch/sidecar/native records. Process capabilities do not survive restart. Tagged original ACKs retain forecast binding checks; old closed untagged ordinary ACKs retain their original native validation. Lost/edited/truncated binding history cannot initialize silently. Unknown acceptance still blocks replacement orders. The SDK's typed original no-admission proof can resolve only the exact proven unsent attempt, without an ACK, fake closure or same-key retry.

## Actual Delaware position

Market353/exchange1042/contract354 is Democratic Delaware Senate YES. Retained official metadata names Democratic Party, General election2026-11-03, race62957/stage98087. The [official candidate list](https://elections.delaware.gov/candidates/candidatelist/genl_fcddt_2026.html) identifies Coons. Retained October15 [Call the Map](https://www.callthemap.com/senate/delaware) and [Who Shows Up](https://whoshowsup.net/race/senate-de) observations report99.5% and above99%, respectively. Those correlated sources do not supply an independent calibration study or complete exceptional resolver proof.

Active review9523497c40e24eb5c66b2251cb59d1c839b874d15fbdcccee4c8c9ea5b254cef6 assumes.95 ordinary probability,.01 exceptional allowance, zero exceptional payout and.01 margin: expected payout.9405, ceiling.9305 before tick flooring. It caps new principal400 and quantity10,000, separately from40,000 total/10,000 per-market limits. Source/review validity ends09:00UTC October16; the original holding deadline remains17:00UTC October19. Neither was extended by incident recovery.

Actual order41846939/q3420 filled430YES at.920, cost395.600000, fee0, executed04:02:17.490UTC and reconciled04:02:36.609UTC, original fill174569344. The preceding q3410 expired without send; it was not reused. The new lot remains open with zero realized profit at04:16UTC. [Actual Paper3 result and limitations](MOLOCH_PAPER3_20261016.md), [FIFO economics](../research/moloch-actual-economics-20261016/README.md).
