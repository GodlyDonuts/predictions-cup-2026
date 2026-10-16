# Original read packets: finite economics and next implementation

Source-only assessment at `ed050666`, October 16. No account requests, SSH,
worker edits, new client, policy changes or runtime operations were performed.
The original finite capture is the one pinned and reproduced in
[the economics audit](../moloch-defensive-cancel-20261016/actual-economics/README.md).
[report.json](report.json) pins the specific retained inputs used here. Raw account
receipts remain private. This is a request-count proposal, not a measured speedup.

The next candidate should reuse **original receipts within their existing TTLs**,
starting with the just-read same-market REST book and complete reconciliation
open list. It should retain fresh fallback reads, all full owned-fill tapes and
physical authority checks. A whole-cycle account snapshot cannot serve an
eleven-minute cycle: its authority expires after twenty seconds.

## What is actually repeated

| Operation | Existing original endpoints | Logical reads before pagination/retry |
|---|---|---:|
| One selected source scope | exchange trades window, market orderbook depth20 | 2 |
| One ordinary preflight packet | tournament, positions, open orders, account, market, nodes, market orderbook | 7 |
| New reviewed fundamental origination | planning seven-fact packet, then FINAL seven-fact packet after reservation/activation; possible preliminary book/tape recovery | at least14 |
| Owned cancellation preflight | tournament, positions, open orders, account | 4 |
| Reconciliation, N>1 pending orders | one shared open list; every order's complete fills; exact order GET for fallback/terminal proof | 1+N to 1+2N |
| Reconciliation, N=1 | exact order, then complete owned fills | 2 |

Source references: `worker.rs:298–338`, `673–683`, `1027–1040`,
`reconciliation.rs:43–105`, `reconciliation_plan.rs:1–5,203–245`.
The source-scope packet itself does **not** fetch four account facts. Those are
added when dispatch nominates a candidate. `dispatch` returns after one actual
submission/cancellation; it is not an unlimited batch submission loop.

Reviewed fundamental origination deliberately refreshes the full packet after
native reservation and activation (`worker.rs:1187–1231`). It may additionally
repair tape coverage and perform the reviewed book prefilter. The final seven
facts were added specifically because planning/waiting ages earlier evidence.
Do not delete that final refresh under an ordinary-cache patch; its opaque
review/plan contracts require an independent lifecycle review. The315-read
example below assumes ordinary preflights, not35 reviewed originations.

`worker.rs:1642–1710` reconciles at cycle start and when its completion-anchored
10-second schedule is due or ACK/terminal counts change. Source refresh, recovery,
optional package work, owned-age refresh, dispatch and Paper5 work are interleaved
inside the selected-scope loop. A read packet is therefore not the sole cost per
scope. `scheduling.rs:94–129` blocks submissions until reconciliation is current.
Changing that interval or dropping fill tapes is outside this proposal.

The reconciler already replaces an exact order GET with a validated member of
the shared complete open list when safe. It still fetches a complete fill tape
for **every pending order**, including unchanged counts. A missing open-list
member is not terminal proof. Its exact-order-then-complete-fills fallback must
remain. Aggregate/member freshness can expire while earlier orders are read;
then exact fallback is intentional, not evidence of redundant accounting.

## Finite original timing, and what it does not identify

For q8178, created17:32:03.612Z, first recorded preflight GET17:33:58.398Z,
ACK17:34:06.060Z:

- Creation to first GET: **114.786s**.
- First GET to ACK: **7.662s**; original book receipt to ACK **1.465s**.
- All recorded spans including policy/durability/ACK application: union3.803s,
  outside spans4.104s. The latter includes pacing and other uninstrumented work;
  it cannot be assigned entirely to any one cause.

Seven original GET spans are present, but the trace does not name their
endpoints. Their existing constructor order is source evidence, not independent
endpoint-tagged timing. The retained `second-preflight.json` is the **startup**
packet at17:07:57–17:08:10, despite its later capture time. Its five original
receipts identify tournament, positions, open orders, account and portfolio
fills. It must not be presented as q8178's fresh seven-fact packet.

The selected list had35 scopes (23 required), at17:24:41.408. Progress reached
32 at17:34:53. This does not provide a complete HTTP endpoint census for the
whole cycle, nor split the114.786s upstream dwell into reconciliation, selected
source work, pacing, stream processing and candidate rotation. No precise
whole-cycle savings can be inferred from these finite artifacts.

Source `worker.rs:42` explicitly requests **100 reads/minute,600ms completion
pacing**. The task asks for60-read arithmetic; both are shown without claiming
the retained actual queue profile was observed. At24 writes/minute,2500ms
completion pacing, contest writes remain shared and serialized. Do not change
the persisted rate profile on the basis of this report.

For S source scopes and P full preflights, baseline minimum reads are `2*S+7*P`,
plus reconciliation/cancel/package/extra-source/census reads and pagination or
retries. Hypothetical S=P=35 gives315 operations, not a measured cycle total.
Rate-equivalent steady-state load is315s at60/minute or189s at100/minute;
rolling-window phasing, completion gaps, service time and other work prevent
these figures being wall-time predictions. Reusing35 genuinely eligible source
books would remove at most35 reads (315→280), with no guaranteed hit rate.

## Minimal future implementation, in order

1. Retain a bounded exact original REST book receipt from `source()`. When the
   selected physical candidate refers to that market, reuse it only if all
   current physical checks pass at dispatch. Both original request and receipt
   ages must be<=10s and the exact exchange's original `asOf.at`<=10s. Verify
   explicit tournament/context/exchange/depth and original native source
   admission; reject gap/reset/invalid source and changed quote. Do not relabel a
   realtime event as a REST receipt. If freshness would expire after subsequent
   facts/queue wait, fetch the final book as today. The earlier source book may
   be too old after reconciliation, so this is an opportunistic one-read saving.
2. Expose the reconciler's **original complete open-list Receipt** as a hint for
   a forthcoming preflight. Revalidate its exact endpoint, tournament, complete
   pagination, known ownership and original request/receipt age<=20s. Keep
   original projection sequence and the planner's no-regression checks. Do not
   synthesize an OwnedOrder receipt from a member. This can save one duplicate
   GET; it cannot replace fills, exact terminal order reads, positions or cash.
3. A bounded in-process account packet can reuse tournament/positions/open-list/
   account originals across write-free candidate deferrals, if **each** is still
   <=20s at the actual HTTP boundary. For K valid preflights in that same short
   write-free interval,7K becomes4+3K (maximum4(K−1) saved). Normal successful
   dispatch ends that reuse interval. Do not assume K>1 or a useful hit rate.

Keep receipt bytes/digests and clocks unchanged. Cache lookup must not bypass
source servicing/STOP/unknown checks. Before preparing the exact spec, reread
the actual current native quote and apply its existing policy tokens. Final
`ProductionAuthority` remains the gate, with fresh fallback on cache rejection.
Do not cache an opaque OrderReady/Permit or reuse logical keys.

## Invalidation and races

| Fact | Conservative invalidation for the candidate cache |
|---|---|
| All four account facts | Any physical POST/DELETE/multi-leg attempt, uncertain outcome, recovery transition, changed account scope/profile/control/lease; expiration or error |
| Cash/positions | Any admitted actual fill, netting, settlement, contribution/collateral adjustment or adoption; exact expected-cash/current-holdings mismatch |
| Open list | Any physical attempt/ACK/cancel/terminal, changed ownership roster, pagination/coverage conflict or projection regression |
| Tournament | Original TTL, status/end-date/scope/control mismatch; no long-lived status cache even if scheduled end unchanged |
| Market/nodes/book | Original10s TTL, contract/status/scope mismatch; book additionally actual version/source invalidity, depth and chosen-quote incompatibility |

For a first conservative implementation, invalidate the entire four-fact packet
on the union of these conditions instead of attempting field-specific projected
updates. Pure book events do not prove a financial mutation, but can invalidate
the candidate and its source proof. They must still be admitted in order.

Native-head equality is **not** proof the venue account stayed unchanged:
resting orders can fill before local observation. Existing separate GETs also
are not an atomic account snapshot. This proposal retains their existing20s
bounded observation semantics, exact bank-vs-position/cash checks and mandatory
owned-fill reconciliation. It adds no snapshot isolation or causal account
barrier. If the application requires a stronger barrier, the current official
schema does not supply one through these four independent endpoints. A newer
GET alone cannot prove that future fills will not occur either.

## Official capability and limits

Primary source: retained official OpenAPI
`api-spec-ioc-20261014.json`, SHA256
`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`.
Pointers are under `/paths` (escape `/` as `~1` in JSON Pointer):

- `/account/get`: explicit tournament balance is cash; availableAdvance is
  lending, not profit. Keep the typed account economics parser. Tournament
  `myBalance` is not a replacement for this exact typed receipt.
- `/tournaments/{slug}/portfolio/positions/get`: open positions with FIFO lots;
  valuation marks are distinct from cash/executable depth.
- `/orders/get`: full authenticated scoped order list with projection coverage;
  pagination does not document snapshot isolation across independent endpoints.
- `/orders/{id}/fills/get`: newest-first page, full lifecycle totals independent
  of page. SDK requests all pages, validates consistent totals and fails conflict.
- `/markets/{id}/orderbook/get`: one market contains all its exchanges, capped
  depth per side; engine `{sequence,at}` must remain original. It is not an
  all-market multi-book endpoint.
- `/exchanges/prices/get`: up to100 exchange prices/top quotes. The existing
  negative-only bulk screen can avoid hopeless packets; it cannot supply full
  orderbook depth, fills, contract nodes or positive execution authority.

SDK typed paths/validation/aggregation: `lib.rs:268–315,332–353`.
Authority TTL/economics/passivity: `authority.rs:34,157–187`. No supported combined
`GET account+positions+orders+rules+books` operation was found. Global
PortfolioFills exists, but current ordinary feedback/terminal authority consumes
exact owned order tapes; substituting a global projection would need a separately
proved chronology/coverage/reconciliation design, not a receipt-cache patch.

## Acceptance checks before any future deployment

Root should claim a narrow worker/cache scope and test original receipt reuse
and fallback under20s/10s boundary crossings; partial fill during reads; cancel
or ACK invalidation; sequence regression; foreign open order; complete-page
failure; STOP/unknown; changed quote; all original hashes/clocks unchanged.
Assert complete OwnedFills still occurs and terminal release still uses genuine
order+tape. Record cache hits/misses and endpoint-tagged spans **within existing
requests**, then compare finite windows without attributing every delay to GETs.
No tests or runtime performance were claimed for this unimplemented proposal.
