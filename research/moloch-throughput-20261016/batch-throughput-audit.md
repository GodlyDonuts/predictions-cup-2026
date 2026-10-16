# Ordinary passive-pair batch throughput audit

Read-only source audit, 2026-10-16. Code baseline `52116911`. No account reads, client, implementation, funding change or deployment was performed for this audit.

**Recommendation:** a two-child ordinary BUY batch is feasible and can halve entry-preflight reads per admitted child. It is not a switch on the existing atomic package transport. The smallest sound implementation needs a batch-specific SDK outcome, an original parent/index dispatch record, and a typed zero-execution release for rejected ordinary children. Do not route passive quotes through the guaranteed-floor/atomic package ledger.

## Primary facts

Retained official OpenAPI: `runtime/moloch/api-spec-ioc-20261014.json`, SHA256 `6b5ff9a661c06f1a2d1dda131bf870a70f4f79699ac9937375a278861e1f81bf`. This retained schema is evidence of the documented API, not a fresh network observation. Relevant objects: `paths./orders/batch.post`, `components.schemas.BatchOrderInput`, `components.schemas.OrderInput`, and `info.description` rate-limit section. The [official supported API base](https://sig.thesuper.market/api/v1) is the venue source; no website-internal interface is needed.

- Request: `{ "idempotencyKey": "stable-parent-key", "orders": [OrderInput, OrderInput] }`. The documented maximum is 50; the proposed first implementation deliberately uses exactly two. Child `OrderInput` has exchangeId, side, action, quantity, optional price/expirationDate/tournamentId. It has **no child idempotencyKey**. Freeze explicit tournament, numeric tick limit and expiration for both children; do not use omitted-price/0/1 market orders or default tournament routing.
- Execution is sequential, best effort. All children are validated before placement, but later placement failures do not roll back earlier placements. The response maps child indexes through `{index,ok,status,data}`.
- HTTP200 means all admissions succeeded. HTTP207 means at least one succeeded and another failed; it does not describe partial fills. HTTP422 means all failed terminally. Individual429 `RATE_LIMITED` means nothing was placed for that index. HTTP502 `ORDER_STATUS_UNKNOWN` and some503 outcomes can leave execution uncertain. A503 execution-budget stop may retain already placed orders and omit unissued work.
- A completed identical parent key/payload replays its stored envelope. An incomplete retry can resume nonterminal indexes, including429/5xx. Therefore a retry is potentially a **new placement at an unfinished index**, not a read-only status fetch. The minimal first implementation must never automatically retry the parent or replace an uncertain child.
- The documented account API budget counts one batch HTTP request as one write. Moloch's existing24/minute write ceiling and completion pacing can stay unchanged. The trading engine has a separate per-order throttle; batching does not remove it. Honor the longest original Retry-After, including on207.

## Current code: reusable parts and missing boundaries

| Area | Current behavior | Required change |
|---|---|---|
| SDK [lib.rs](../../backend-rust/transport-service/src/lib.rs), `place`, `perform` | Typed single `/orders`; typed atomic `/orders/multi-leg`; no ordinary batch method. One parent queue binding can already bind a full scope/request/body hash. | New typed exactly-two batch method/validator. Retain raw response bytes for **every status before parsing**, with original request/clock evidence. |
| SDK completion classification, `perform` | A valid2xx normally completes as success; malformed2xx becomes unknown;5xx normally unknown. `last_basket_response` is specific to atomic multi-leg. | Inspect every child even on207. Preserve accepted IDs while marking any unclassified index unknown. Do not let a partial-success207 silently clear unknown acceptance. |
| [authority.rs](../../moloch/live-runtime/src/authority.rs), `prepare_with_policies`, `check` | Holds one `Ready::Order`; final ordinary method/path must be POST `/orders`; one frozen native quote/permit/fact packet. | Opaque `Ready::PassivePair`: exact two native quotes, original shared seven receipts, full parent bytes/index mapping, combined funding and individual clock/price checks. |
| [dispatch.rs](../../moloch/live-runtime/src/dispatch.rs) | One qid per intent/attempt/ACK; unresolved attempted qids block new submissions. Explicit no-network proofs do not mean an HTTP-rejected order. | Compound parent intent/attempt records must durably cover both qids before one physical call; explicit original accepted/rejected/unknown outcome per index. |
| [engine_live.rs](../../moloch/native/src/engine_live.rs), ACK/fill/terminal | Ordinary qids already reserve independently. ACK binds identity/admitted quantity, never fill profit. Unsubmitted retirement requires zero network attempts. Terminal requires actual binding and complete reconciled fills. | Reuse real ACK/fill/terminal accounting for accepted children. Add narrowly verified ordinary rejected-child release; an HTTP-attempted rejection cannot be called unsubmitted or receive a fake ACK/terminal. |
| [feedback.rs](../../moloch/live-runtime/src/feedback.rs), startup/reconciliation | ACK helper expects original `/orders` response data and status200. Recovery and close use exact known actual IDs/full tapes. | New indexed ACK normalizer references the **original batch receipt** and index. Never fabricate `/orders` child receipts by changing endpoint/status/body. Startup must understand original batch evidence. |

Atomic package infrastructure supplies patterns for original descriptor pinning, first-call tickets, raw response evidence and crash-safe outcomes. Its current reservation class, floor proof, full-fill ACK and `NoExecution` semantics are incompatible with two independently resting ordinary quotes. Reuse utilities only, not a false package classification.

## Minimal eligible pair and physical sequence

1. Select exactly two **existing, unattempted, individually funded native BUY qids**, same exchange/market/tournament, opposite YES/NO. Both must be active, uncanceled and unexpired; each full native quantity/price/expiry remains immutable. Exclude package and dedicated forecast-managed markets, owned inventory and other unresolved target execution in this first version. If only one eligible qid exists, retain today's single path.
2. Obtain the existing complete original seven-operation packet once: tournament, positions, open orders, cash, market, contract, full book. Admit that original book and re-read both exact qids. If either changes/cancels, do not substitute a different qid or slice. Independently apply current scope/rules/fees, global unknown, account holdings/cash, contract and book freshness, activation/entry cutoff, passive limit and per-order risk checks to both.
3. Verify both native reservations still exist; sum worst-case BUY principal is already reserved and fits the same40k account/10k market caps. Credit no future pair redemption, partial execution, sale, collateral or expected profit. Keep individual quantities unchanged. Conservatively require the pair's aggregate quantity to fit the current parent permit maximum until an explicitly reviewed per-child-cap batch permit exists; do not silently turn the10k quantity cap into20k.
4. Freeze parent key, ordered child bodies, `index→qid`, exact native head/report, request hash and shared facts. Both passive prices must remain safe if only that child is accepted/filled. `YES limit + NO limit < 1` prevents the two new quotes crossing each other, but does **not** guarantee that both fill or create a new arbitrage certificate.
5. Append a typed compound intent and compound attempt before the SDK call. A crash must never leave one child physically eligible but absent from durable attempted state. Bind a permanent one-call SDK ticket to this parent key/body. One queue admission/network event covers one actual HTTP request; child attempts refer to that parent rather than fabricating two network requests.
6. At the final physical boundary recheck the same original control/STOP/binary/lease/stream/queue/permit clocks and both frozen qids, limits and fact freshness. If SDK waits make the packet stale, stop before HTTP. A batch introduces no cache exemption.
7. Preserve the complete original response, then validate unique indexes, known index bounds, exact child exchange/action/side/limit/admitted quantity, and distinct actual order IDs. Store all known accepted IDs even if another index is malformed/unknown. Apply original source-bound native ACK events; subsequent fill/cash accounting uses each original official complete fill tape, never admission totals.

## Outcome/restart rules

| Original outcome | Durable/native handling | New writes |
|---|---|---|
| Two canonical accepted children | Bind both actual IDs; record no fills from this response; normal per-ID reconciliation/cancels/expiry. | Existing global guards apply. |
| Accepted + explicit terminal rejection or per-item `RATE_LIMITED` | Preserve accepted actual ID and reserve. Rejected child can release only through the new typed original no-execution proof + permanent no-parent-retry fence. Honor backoff. | Only when every index is economically classified and queue globally quiet. Server resumability is not permission to retry. |
| Accepted + missing/ambiguous/unknown child | Keep accepted-ID full-tape reads/cancellation available. Unclassified child's full reservation remains. Global unknown remains unresolved; do not drop the accepted child simply because parent is unknown. | Block. |
| Complete authoritative all-zero result | Release only verified rejected qids; append no ACK, fake closed order or fill. Exact duplicate replay is idempotent. | Existing backoff/guards apply. |
| Timeout, incomplete body, malformed successful envelope,503/502 without complete definitive index evidence | Retain original bytes and whichever indexes/actual IDs can independently be verified. Missing receipt/index or apparent open-order absence is not nonexecution evidence. | Block; no blind parent/child replacement. |
| Crash after response retained but before either/both ACK commits | Reopen the original parent receipt/index map; verify exact bytes/head/request and idempotently bind/apply only missing children. | Block until classified; known IDs continue reconciliation. |

The initial rejection allowlist should be small and code-based: authoritative full upfront validation failure or exact per-index documented zero-execution failures. Generic4xx/409, message text, missing indexes, a locally empty queue or outer207 alone is insufficient. An unresolved original batch can require operator reconciliation; ordinary open-order absence cannot reconstruct a lost batch's identity.

## Admission-count upside and practical limit

For two independently submitted qids, today's entry packets cost14 reads plus two writes. The pair batch costs7 reads plus one write, before source/reconciliation/cancel work. This is a deterministic **50% reduction in entry-packet reads per child**, not measured latency or expected fill gain.

If `R` is other reads/minute, `W` other writes/minute and `E` permitted child admissions/minute under the separate engine throttle, pair rate is bounded by `min((100-R)/7, 24-W, E/2)`; admitted-child rate is at most twice this, before eligibility/rejection. Ignoring all other work gives an unattainable-in-practice read upper bound of28.57 children/minute versus14.29 singles. Do not claim48 child orders/minute from the API write ceiling alone.

Full per-ID fill-tape reconciliation, cancellations, source traffic, usable passive-pair availability and actual order flow still dominate sustained turnover. A batch may admit two nonfilling quotes faster. It creates no alpha or fill probability.

## Proposed implementation split and tests before activation

Root can approve separate narrow scopes: SDK `ordinary_batch.rs` + `lib.rs` raw-response/classification hook; runtime `ordinary_batch.rs` coordinator and compound `dispatch.rs` records; `authority.rs` pair capability; `feedback.rs` indexed original evidence; native rejected-child module/routing; worker/startup/reconciliation integration. Keep original single behavior and old journal replay unchanged.

Required meaningful tests use real Session reservations, original dispatch files and loopback SDK responses: both accepted; one accepted/one terminal rejection; one accepted/one429 plus backoff; accepted/unknown; all429;503 after an earlier real admission; malformed/duplicate/missing indexes; canonical side/price/quantity change; duplicate/conflicting actual IDs; body-read failure; crashes before HTTP and after each durable response/ACK boundary; restart with one already applied ACK/fill; unrelated global unknown; STOP/stale shared facts; exact combined capital/per-market/quantity caps. Assert no inferred fills, no rejection release for uncertain indexes, no retry/replacement, original ledger parity and actual read/write admission counts.

Until those boundaries exist, do not activate batch placement. The already checked book-first filtering and funded breadth improvements are independent and need not wait for this work.
