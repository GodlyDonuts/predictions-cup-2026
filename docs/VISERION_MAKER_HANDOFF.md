# Market-maker integration handoff — October 12, 2026

Owner: Viserion, chat `01a12eff-035b-77e1-b89a-dc102c3111e9`.

The human authorized paper operation and said: “You can run the Bot Baal in production as soon as it shows signs of profit.” This supersedes the previous lack of permission for a new activation; it does not establish a profitable strategy, renew an old activation, resolve outstanding orders, or provide a second account-worker lease.

## Current state

**STAGED.** The components below are published and checked. No live collector/host imports them as of main `30d6c334`. No orders were submitted by this handoff. Rhaegal reported the existing collector and paper worker live, with `EXECUTION_STOP` still present; that remote process state has not been independently verified here.

- `45779f61`: `execution/maker-quote-scheduler-v1.mjs`. Selects cancellations or up to eight funded, ranked quotes for one write request. Preserves unchanged queue positions, prioritizes inventory exits, avoids complementary self-crosses, and freezes writes while acceptance is unresolved.
- `787ca019`: `execution/maker-batch-outcome-v1.mjs`. Binds the explicit tournament and immutable ordered batch/key. Retains returned order IDs, including canonical mismatches, and distinguishes terminal rejection from missing, transient, or unknown indexes. A placement response never establishes fill/cash reconciliation.
- `30d6c334`: `execution/maker-funded-allocation-v1.mjs`. Sizes scheduler inputs against the financial owner's spendable cash **after all existing reservations**, attributed inventory, and committed-principal limits. Never releases a pending cancellation's reservation or pledges shares to both sale and complementary purchase.

The existing `paper/inventory-aware-quote-v1.mjs` already provides research inventory skew. Its microprice is a heuristic, not a calibrated conditional-on-fill value forecast. Do not turn a quoted spread into an asserted expected profit.

## Operating consumer

Integrate inside the existing financial owner and serialized account client; do not launch `Start-Paper.ps1` here while the other collector owns account reads.

1. Reconcile complete cash, attributed inventory, open orders and recent fills through the existing worker. Supply exact spendable cash from the shared budget, preserving every other strategy's reservation.
2. Generate fee-aware desired quotes from the selected maker policy and fresh executable books. Pass them through `allocateFundedMakerQuotesV1`, then `planMakerQuoteV1` with matching quote policy and the current shared budget hints.
3. For an action, persist `beginMakerDispatchV1` before dispatch. Bind batch bodies with `bindMakerBatchV1`; use exact owned-order cancellation for `cancel` decisions. Extend the host's permit/allowlist explicitly; the existing transport currently refuses batch endpoints.
4. Preserve account and financial-owner leases, STOP/activation checks, immutable payload binding, durable per-intent funding, and both pre-dispatch/pre-network guards. One batch is one shared write admission, not an independent limiter.
5. Inspect each batch result. Reconcile every accepted/unknown order identity, remaining quantity, fill and cash effect. Incomplete retries retain the identical ordered body/key; never replace an unknown batch with new keys. Call `finishMakerDispatchV1` only after the owner establishes the exact outcome and fresh complete account projection.

## Profit evidence and initial production

Use a separately frozen prospective paper cohort, preserving old histories. A first bounded production cohort may start once later-source closed maker round trips show strictly positive net realized profit **and** the cohort remains net positive after executable liquidation of its remaining inventory, fees and modeled exit costs. Unresolved fill/coverage liabilities must be accounted for, rather than discarded. No additional arbitrary trade-count threshold is imposed. Synthetic unit tests, overlapping opportunity totals, favorable provisional marks and conditional settlement surplus do not satisfy this condition.

Paper fills remain counterfactual queue assumptions. They support a bounded first production trial; they do not establish scalable execution or profitability. Record the frozen policy/code, original source prefixes, exact FIFO costs, closed fills, remaining inventory/depth, fees and observation UTC time before activation.

After the condition is met, create a new named activation with fresh complete official reconciliation and the single owner's lease. Initially retain the existing limits: maximum 1 SUSQie per order, 10 aggregate attributed cost, 25 cumulative purchase principal, 3 markets, 60 reads/24 writes per rolling minute, and 2500 ms write completion pacing. Keep the operator STOP until that owner completes activation/reconciliation. Do not count other traders' holdings as Baal fills. Measure actual closed net returns and outstanding exposure before increasing deployment.

## Checks

```powershell
node --test execution/maker-quote-scheduler-v1.test.mjs execution/maker-batch-outcome-v1.test.mjs execution/maker-funded-allocation-v1.test.mjs
```

31 checks passed before publication. The offline shared-client integration check proves eight requested orders use one physical write admission and both guards. It makes no claim of a live fill, runtime deployment or profit.
