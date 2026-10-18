# Tartarus managed inventory and conditional rotation — candidate

This is development status, not deployment or profitable rank execution evidence. Production remains R39 worker735621. The latest verified real result is the existing fundamental exit of10,000 exchange1067 YES at0.33, with186.23 FIFO gain; see [original verification](checked-production-lane/live-fifo-fill-verification.json).

Candidate source: [b413fabd](https://github.com/GodlyDonuts/predictions-cup-2026/tree/b413fabd1b59b3efe595e0539afd5a479bc0b0f5) on `codex/tartarus-managed-continuation-20261018`. The combined281-input candidate is in ARM64 qualification. It has not passed the full integration/release checks and is not deployed. Checked direct-main source publication remains required after those checks; no PR or force push is used.

## Accountable controller handoff

The old fundamental controller must not treat a rank-risk trade as a positive-monetary-alpha order. The candidate adds an opaque `RankInventoryOwnership` capability, separately from the fundamental value decision and the rank evaluation. The original held Cohorts writer registers each exact request and retains its old entry/adoption bindings and original lots.

The first transfer requires a covered original-side SELL. It freezes the original cohort, target FIFO position, prior fill prefix and native head. Subsequent registered rank BUYs or SELLs are reconstructed from those original lots and actual admitted fills. An opposite-side BUY is possible only after the original opposite inventory is completely gone. Foreign/unregistered fills remain errors. Original costs and historical profit attribution are never relabeled.

The original controller pauses new actions for transferred scopes. That pause survives nomination expiry and process restart; it does not rearm an old forecast. Closed/unknown attempts do not authorize another submission. Only an unchanged, never-attempted binding can resume. Existing ACK/fill reconciliation remains on the same account worker.

The nomination's optional `managedInventory` contains `schemaVersion`, `exchangeId`, `generationSha256`, `reviewSha256`, `cohortSha256`, `originSha256`, `exclusionSha256`, `positionsSha256`, `handoffEstablished` and `sourceSha256`. Its compact ownership statement is a retained source; it is not probability evidence. Native preview checks its hashes and current target position. Physical minting additionally requires the non-deserializable capability from the original sidecar/OS owner, current whole-account facts, current contract/book evidence and independent owner-approved nomination pins.

A fixture exposed a genuine trial-application bug: the new ownership map was initially not restored alongside the old maps before the durable append. The candidate now restores it in the same rollback. This correction is being rechecked; production was unaffected.

## Funded conditional multi-sale campaigns

`contingentPlan.tradeLegs` adds explicit BUY/SELL actions alongside the legacy BUY-only `buyLegs` format. Providing both formats is rejected. At most128 future physical tranches are modeled; their pacing, source clocks and expiry bounds remain explicit.

A separate cloned FIFO ledger consumes each modeled sale once and carries the remaining original principal. Buys use only the cash available at their modeled prefix. Every lot boundary is evaluated for refund/downside and partial-fill behavior. Repeated sales cannot duplicate inventory; pending/acknowledged sales never credit actual cash. Market, committed-principal, quantity and fresh-market limits still apply. Future controller transfer, liquidity and completion remain conditional. Every actual next leg requires real fills, real cash, fresh facts and renewed concrete review.

The new tests include two-lot sales, source/account integrity, loss realization, partial fills, unchanged-binding reuse, restart recovery and a full original-side liquidation followed by an opposite-side purchase. They also check that a projected sale funds only later modeled tranches and creates only the currently admitted physical reservation.

## Research view without a second collector

`tartarus-current-native-state.json` is produced at completed worker scopes. Schema: `tartarus_native_research_view_v1`. It contains derived native cash/reservations/positions/fill hashes, actual policy, compact ownership statements, active orders and retained books with their original clocks. Transferred scopes remain available after their holdings reach zero so the planner can evaluate a subsequent purchase.

This writer receives no Transport or mutable Session. It creates no SIG request or financial event. Original receipt clocks are retained separately. It explicitly declares that it is not an atomic official account snapshot, complete current universe, checked SDK boundary or execution capability. A file write time never becomes a market/source publication clock. Current physical preflight remains mandatory. No credential, control budget, daily window or STOP is altered.

## Adverse field retention

An explicit `risk.targetGainAggregation: pareto` option retains every declared probability/field pair. The candidate must not reduce the modeled strict first-place probability in any retained pair and must improve it by the reviewed minimum in at least one pair. A high adverse threshold can therefore remain visible with zero modeled hit probability instead of being silently omitted. A candidate with zero improvement in every pair is rejected. The legacy all-pairs improvement rule remains the default.

These probabilities are declared model outputs, not calibrated win probabilities. The monetary losses, failed continuation branches and unmodeled-state statement remain required. This option does not manufacture attainable depth or promise a first-place outcome.

## Remaining production work

The combined candidate must complete integration/release qualification and independent source/artifact verification, then the existing funded bank must migrate without changing historical financial state. Current account/debt/semantic/depth inputs, coherent joint scenarios, threshold sensitivity and a source-qualified feasible campaign still need to feed the nomination. No standing policy, order approval, actual Tartarus trade or massive ROI is established by this branch. Empty scopes that have never had an original cohort still require an explicit ownership transition; they cannot be adopted by fabricating a zero-cost position.
