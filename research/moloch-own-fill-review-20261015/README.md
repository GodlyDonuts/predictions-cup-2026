# Own-fill economics: exit draft starvation precedes strategy tuning

Task [#130](https://github.com/GodlyDonuts/predictions-cup-2026/issues/130), read-only session`Viserion/ordinary-fill-review-20261015`. The actionable finding is **a short inventory-exit draft can expire before production's authorization packet completes**. Increasing the inventory holding period or fitting a model from three clusters would not repair that execution mismatch.

## Authoritative source window

`export.py` read the existing VPS files through SSH CLI only, at nice10. It made no account requests and wrote only a new private directory outside the runtime. It verified the sealed manifest/report and every native journal row's chain/hash through sequence10176,25,749,909 bytes, prefix SHA256`a4e0e7a07551c610d67b0fefb44e1ecbea44d6947614fa85d3f10b304b4a580b9`, tail`8f59fb10e14416d12b01305fc3a9e80ee28d86a8e00ba963aa25c53e1c9045be`. Report clock: **2026-10-15T10:01:10.493Z**. The private filtered export contains627 events and17 native actual fills in the three new-entry exchange clusters. Raw account/portfolio receipts are not published.

`results.json` records source-prefix, filtered-export, manifest, report, dispatch and code hashes. The local audit verifies the filtered byte hash and exact native actual-fill identity/price/quantity/receipt links. It does not independently re-fetch or re-prove every raw API page's completeness. The native production feedback already required owned-order and complete-fill provenance before admitting those events.

## Actual economics, with inherited gains separate

| New-trading quantity/economics | Measured result |
|---|---:|
| BUY shares / gross principal |385 /369.200000 |
| SELL shares / new realized return |22 /−0.110000 |
| Remaining new shares / entry basis |363 /348.300000 |
| Own-entry order/exchange clusters |3 |
| Inherited realized gross, separately reported |+11.220000 |

The only closed new cohort bought22YES at0.950 and sold22 at0.945, zero reported fees. Actual execution-to-execution holding time was **282.479 seconds**, not60 seconds. The position's maximum-age SELL intent was recorded09:03:07.090Z and its fill executed09:03:08.053Z. No new-trading profitability is demonstrated, and inherited gains must not conceal the new strategy loss.

All16 new BUY fill rows were YES, across just three orders. Their fills were known to the native worker11.429 seconds after execution in the first cluster; the second cluster's delays ranged33.084–579.292 seconds; the third59.905–318.178 seconds. Multiple fills of one order are not independent calibration examples. The current passive pair policy therefore has insufficient own-fill evidence for the Paper1 HJB/fill-hazard calibration requirement.

## Exact clock distinction

The saved primary API schema`BookVersion.at` says **“The engine clock when the book was read.”** `sequence` identifies the last applied engine event. `at` is not documented as the last price-change time. The audit inspected the official saved schema with SHA256`6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`; public reference is [SIG API docs](https://sig.thesuper.market/api/v1/docs). No fresh account call was made. No same-sequence/different-read-clock pair was present among these34 REST events, so such an empirical example is not claimed.

In native`Engine::apply`, decision time is the event's `processedAt`. `Book::clock()` uses:

- **WebSocket:** `book.sourceAt`, normalized from engine`asOf.at`; `requestStartedAt` is set to local broadcast receipt and is not an HTTP request.
- **REST:** actual `requestStartedAt`; the book still retains its separate engine-read`sourceAt`.

`Book::fresh` requires not-invalid state, receipt no later than processing, and decision time minus the selected clock at most10,000ms. The guard is appropriate and is not weakened in this work.

For the three entry scopes only:

| Book path | Events | Not invalid | Fresh at processing | Fresh at receipt, lost during local processing |
|---|---:|---:|---:|---:|
| REST |34 |25 |25 |0 |
| WebSocket |490 |490 |320 |165 |

REST receipt→processing median34ms, p95/max119ms; engine-read→receipt median85.515ms, max157.978ms. Thus **this sample does not show unchanged old REST state clocks blocking fresh observations**. WebSocket receipt→processing median6.832s, p9518.952s, max24.422s. Some broadcasts were already late when received;165 additional books crossed the freshness limit during local processing. These are selected-scope measurements, not the whole tournament or an attribution of provider faults.

One direct trace: native draft255 was created from a fresh REST book at09:51:53.338Z. It activated09:51:56.338 and expired09:52:06.338. The following stream drain was still processing valid-but-stale source books at09:52:09.968 (receipt09:51:59.446, engine read09:51:59.158800600). That event correctly fails freshness:10.8092s old. The original draft had already expired before the drain completed.

This does **not** mean every missing exit was caused by stale books: fresh REST observations did create exit proposals. Native source recovery also emits new gap events and requires book/tape recovery; newest cached state is not permanently invalid, because fresh recovered REST books exist in the same prefix.

## Source-backed execution mismatch

`engine.rs` creates inventoryAON drafts with expiry`created + latencyMs + bookAgeMs`:3,000+10,000=**13,000ms**. Ordinary resting quotes use the longer600,000ms lifetime. All22 maximum-age SELL drafts in this prefix had the13s lifetime and3s activation delay.

The original worker's dispatch loop skips a draft until activation and requires more than5s to expiry before starting preflight. It then performs six serialized GETs through`snapshots` (account, positions, open orders, market, contract, book), feeds the fresh book into the native state, and requires more than3s remaining before authorization. Only **one of22 maximum-age drafts reached a physical intent**;21 have a source-bound dispatch`not_sent` record, and no corresponding physical SELL intent. The later two entry clusters generated17 age-exit drafts and zero physical exit intents.

The records establish draft expiry/starvation, not the exact internal branch for every historical retirement: the old `not_sent` record does not preserve preflight-phase timing/reason. Therefore the six-GET bottleneck is a causal code path plus measured feasible timing, rather than a claim that every draft entered all six reads before expiring.

## Smallest beneficial change and regression

The parent's proposed **read prefetch** is the preferred first correction: for a **never-attempted, no-physical-intent, maximumAge inventoryAON SELL** only, permit gathering the same facts up to3s before activation. Keep final native quote re-read, exact quantity/price/expiry, actual activation, Authority, account/book freshness, owned inventory, STOP, stream and unknown-acceptance checks. It creates no early trading authority and extends no quote or order expiry.

Concrete regression: draft created0ms, activation3000, expiry13000; preflight7500ms. Waiting for activation finishes at10500, leaving2500ms and failing the final3000ms fence. Starting reads at creation finishes7500, leaves5500ms and passes the unchanged activation/expiry fences. A fast2000ms preflight must still not dispatch before activation; an11000ms packet must still fail the expiry fence. Tests cover these boundaries.

Prefetch alone cannot recover a draft already aged beyond13s in a large stream drain. Do not move general dispatch ahead of unprocessed gap/financial events or relax freshness to mask that. Measure quote-created→prefetch-start, preflight duration, and terminal deferral reason. If batch starvation remains, the next bounded design should interleave stream draining between already serialized read stages, or explicitly rebuild **only never-attempted** expired drafts after the final fresh book with a new identity and full unchanged authority. It must preserve all trades/revisions/gaps and bank event order; blindly dropping old broadcasts is not equivalent.

## Own-fill markouts and limits

Fixed0/30/60/300s-after-knowledge labels use the first fresh processed same-exchange book within a10s lateness window and walk full depth. Only1/1/1/0 of16 BUY fill rows qualify. The sole60s observation marks−0.110; later two clusters are censored, not zero-return samples. Independent liquidation labels are not actual fills or a funded counterfactual portfolio. No interpolation or favorable later peak is used.

No model, age threshold or alpha parameter was fitted or selected. Paper2's relationship/collateral approach remains a different strategy lane; ordinary forced selling cannot turn a merely conditional structural package into market-making profit. Repairing draft serviceability makes the requested strategy executable; it does not establish positive expected value.

## Reproduction and checks

`node --test research/moloch-own-fill-review-20261015/audit.test.mjs`:5/5 pass. Cases cover the actual late-book clock distinction, invalid-state preservation, prefetch timing without early dispatch, depth/NO conversion and exact own-fill FIFO accounting with unbacked-sell rejection. This audit is restricted to the observed zero-fee cohort and rejects nonzero fees pending an explicit allocation adapter.

On the VPS, `nice -n 10 python3 export.py /absolute/live/root /new/private/directory` creates a source-bound private export without modifying source files. Locally, `node audit.mjs PRIVATE_EXPORT_DIRECTORY NEW_OUTPUT.json` refuses overwriting results. No native/shared code, account client, production policy, activation, journal, holding or runtime was changed by this task.
