# Native reservation and physical throughput audit, October 16

Read-only observation of worker421878/source5577e984. The authenticated report boundary is sequence195620 at **2026-10-16T08:18:34.053Z**; supporting files were copied08:18:37–08:19:23UTC. These independently changing files are a bounded observation, not an atomic live snapshot. No account API, queue command, live Session, service or control modification was used. Original private evidence remains outside Git.

**Dormant native BUY drafts dominate reservation usage, while actual passive orders are frequently submitted late in their original ten-minute lifetime.** This is not evidence that physical liabilities can be released. All actual ACK/fill/terminal ownership remains authoritative.

## Exact reservation composition

The report has cash17,194.180000, owned principal22,731.515000 and native reserved cash15,605.600000. Cash less native reservations is1,588.580000; the unchanged40,000 total-commitment cap has1,662.885000 headroom. Its30 nonretired native orders split exactly:

| Native orders | Count | Reserved cash |
|---|---:|---:|
| Never-attempted ordinary flat-scope BUY drafts |14|13,616.790000|
| Actually ACKed BUYs without original authoritative closure |4|1,988.810000|
| Never-attempted covered SELL drafts |12|0|
| Total |30|15,605.600000|

Thus87.26% of reserved cash belongs to genuinely never-attempted BUYs at this captured boundary. SELLs pledge inventory and consume selection/read attention, but do not explain reserved cash. None of these30 records has reviewed-entry or package metadata. The18 BUYs cover10 currently flat exchanges, so they are not held-inventory opposite-BUY hedges. The four physical pending qids are4775/4776 (1034 NO/YES),4803 (1007 YES) and4809 (1033 YES). Their unsent opposite-side drafts must not be confused with isolated inactive markets: a later partial physical fill can change the inventory/hedge context.

Largest never-attempted reservations:

| Native qid / exchange / side | Quantity / limit | Reserved cash | Age / lifetime left at boundary |
|---|---|---:|---|
|4810 /896 /NO|8,083 @.660|5,334.780|253s /347s|
|4826 /1016 /NO|4,867 @.935|4,550.645|130s /470s|
|4799 /891 /NO|1,313 @.870|1,142.310|307s /293s|
|4830 /1033 /NO|1,356 @.670|908.520|64s /536s|
|4824 /865 /YES|879 @.840|738.360|146s /454s|
|4802 /1007 /NO|498 @.685|341.130|285s /315s|

The first two reserve9,885.425000, or63.35% of all reserved cash. Their original terms are fully funded, not evidence of a present executable edge. Q4826 is one tick behind the **last observed** NO bid (.935 versus.940); that source is28.553s old and cannot certify current competitiveness. Q4810 remains at its last observed NO bid, so a proposal to release every large draft as “bad” would be unsupported. Changing visible depth also does not authorize resizing an original order.

## Why native drafts do not promptly become physical orders

At source5577e984, `native/src/engine.rs:1129–1194` ranks flat entries by spread and historical print quantity, with a positive `ln(activity+2)` term even when activity is zero. `engine.rs:1268–1409` funds native targets immediately from current internal cash/headroom and same-side BBO depth, before any physical attempt. Paired sides can acquire different quantities after later revisions. Native planning is distinct from the runtime's observed-print **read nomination**; increasing discovery reads does not force the same market to win physical dispatch.

`engine.rs:1213–1240` keeps a covered flat-scope order when its book is stale but still within source-silence tolerance. `desired()` first rejects stale books (`engine.rs:834`), so stale data does not establish a replacement price. Remaining native reservations therefore persist until original expiration or a genuine cancellation/retirement path, rather than disappearing when dispatch cannot use them. The source emits104,270 cumulative `writeBudgetHolds`; these are native modeled planning/cancel holds, not104,270 contest HTTP writes.

The08:18:26 quote-priority diagnostic assesses27 then-never-attempted records:21`needs_refresh`, four`competitive`, one`deferred` and one`risk_exit`. Three physical orders already existed before its final fourth ACK. At the sealed08:18:34 report boundary only the two1034 quotes have a last book within10s; other active orders' retained books are16.9–30.6s old. No new financial freshness assumption can be made from their last prices.

The sole worker preserves one expensive preflight per pass. `worker.rs:1291–1325` performs the selected scope's original tape/book source refresh, drains source, reconciles if due, then runs global dispatch. `worker.rs:1035–1076` selects through priority classes and the persistent market/side/action cursor; it need not choose the scope just refreshed. `worker.rs:897–917` reads a real book for `NeedsRefresh`, then still requires the independent seven-fact physical packet on a positive screen. Cancellation/fundamental/covered exits and uncertainty guards remain ahead of ordinary entries. This protects correctness but means a draft can spend minutes waiting while keeping funding reserved.

## Actual placement delay and observed demand

The captured original dispatch chain has29 new ACKs since the07:43:57 launch: **29 BUYs, zero SELLs**, with actual total fill count unchanged at132. Median native creation-to-ACK delay is164.043s;10/29 exceed300s and five exceed500s. Examples:

| qid / exchange / side | Native age at actual ACK | Original resting lifetime left |
|---|---:|---:|
|4719 /1007 /NO|596.954s|3.046s|
|4702 /1000 /NO|589.934s|10.066s|
|4667 /1015 /YES|583.586s|16.414s|
|4746 /1016 /YES|565.235s|34.765s|
|4776 /1034 /YES|462.081s|137.919s|

Those deadlines were never extended; the short residual lifetime materially limits fill opportunity. An ACK is not a fill or favorable demand. Among550 deduplicated actually admitted public prints retained in the verified suffix,27/29 orders have no observed same-exchange print after ACK and before original expiry/report boundary. Q4723 NO896 has nine and q4809 YES1033 one; **none of29 has an observed touch or strict-through at its frozen BUY limit**. These retained normalized observations are not an independently complete venue tape and cannot prove venue-wide zero activity, queue priority, fill probability or positive EV. They nevertheless provide no observed demand capable of explaining a missing owned-fill credit.

## Covered exit gap and minimal remedies

Eleven active never-attempted inherited SELLs have fixed-gain floors above their last observed ask; they account for read/selection work, not the cash reservation. One genuine native profitable-age SELL is different: q4835 sells2,448 inherited NO948 at.900 against7,603 held shares at.895 basis. It is `inventoryAON` **without** `inventoryExitReason:maximumAge`; this is a profitable-age source-bound exit, not an invented new-entry loss timer. Created08:18:23.456, its original13s lifetime has2.403s left at the report boundary. The inherited lot has no new `executedAt`, so the existing new-lot-only age-read scheduler cannot nominate it after its scope was passed. `inventoryAON` describes the native target; ordinary physical POST/orders does not promise atomic venue execution.

1. **Concrete runtime candidate `21e07641`:** extend only `owned_age_reads` to nominate an authentic nonretired, uncancelled, activated, still-unexpired covered native `inventoryAON` SELL, with positive actual same-side ownership and explicit `sourcebound_covered_exit` reason. Keep managed/package protection, current-scope exclusion,30s actual-read cooldown and shared alternating round-robin. This grants one source refresh only. It neither creates/reprices a SELL nor extends its expiry or bypasses physical facts. The original new-lot age path retains a distinct reason. Parent owns tests/integration/deployment.
2. **Proposed local draft cleanup, pending separate checked scope:** select only ordinary flat-scope BUY drafts at least60s old, genuinely never attempted and without durable intent/ACK/binding/fill, outside reviewed/package ownership, with current strict native priority `NeedsRefresh/current_book_unavailable`. Preserve any same-exchange unclosed actual/uncertain attempt to avoid discarding a pending leg's cover. Bind selection to exact current native head/quote and original dispatch state; the owner may then use its existing durable positive never-sent proof and `unsubmitted_intent_retired` with planning deferred. No physical order or unknown liability is released, no official expiry is renewed, and the actual cash balance remains unchanged. An intervening event invalidates the candidate. Do not label all13.6k presently releasable:1,249.65 belongs to unsent mates of current pending physical orders, and current clocks/state must be rechecked.
3. **Further selection work requires evidence:** prefer serviceable current/active opportunities and leave enough original resting time, while retaining fair reviewed reads and urgent genuine exits. Do not force negative-EV crossing, invent a flow/fill model, or remove the seven-fact/owned-fill safeguards to inflate transaction counts. A blanket shorter native order lifetime could make the measured late-admission problem worse.

## Original-source verification

The report seal verifies as `3e143e2f3c271cea76b04400693c96e0caae7c7d45d9f1227a49ad1feefba3d3`. Its raw1,527,740bytes SHA256 is `7a537bf04f85ff6f19ec0ff3a60fb5554368e183270bd4ec4032cd0e86c1d8a8`. All8,065 original dispatch rows were hash/sequence/previous-chain checked; raw4,061,431bytes SHA256 is `86cb7ff2fffa0a344deef60665f5bfc6f5ee6796c85567a718b2c2dc9cee6c74`. All8,952 complete original bank rows186669–195620 were independently hash/chain checked and include the exact captured report boundary. The32MiB copied suffix SHA256 is `7f08073bab749e237b153497f5edfd27aaf5e170e98a7902b63079fcbf1c39fa`; only the portion through the authenticated report boundary was analyzed. All observations retain their original clocks. The separate derived diagnostic files have their own recorded copy clocks and do not replace native/physical authority.
