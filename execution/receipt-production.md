# Native policy matching the frozen receipt paper strategy

`probability-receipt-native-v1` uses a separately built Rust policy and a
schema2 `baal_probability_receipt_production_v1` bank. The original settlement
policy, default binary, active production bank, expired entry window and all
wallet journals remain preserved.

The new policy buys at most one share, uses one-percent sampled depth, adds one
adverse price tick and no fees, requires a0.020 lower-scenario entry edge and
at most0.020 spread. It proposes a sale at acquisition basis plus0.020,
basis minus0.050, or after172800000ms. All thresholds use the adverse-tick
bid, not the raw bid. Research expiry blocks entry while attributed risk can
still be managed. A stop or timeout latches durably even with insufficient
depth, survives restart and remains active after price recovery. A completed
sale starts a900000ms cooldown. Holding age follows the reconciled fill clock.

`prepareReceiptProductionBank` binds the original paper bank, policy, config,
research clocks, source hashes and exact contract. It produces a candidate;
it does not write an activation, reset inventory or place an order. Production
retains the tighter25 cumulative purchase cap versus paper100. Live placement
uses the existing serialized controller, named human authority, fresh contract
and account reconciliation, durable intent, two admission fences and exact
idempotency recovery. Official fills, rather than the counterfactual latency
wallets, determine official P&L.

The checked Kansas candidate is in
`data/receipt-production-candidate-20261005.json`. At12:48UTC the new native
binary evaluated actual subsequent public depth against the existing paper
holding: HOLD, full one-share liquidation0.330, cost0.345, estimated P&L−0.015.
This file-only diagnostic made no API request or contest order. It establishes
policy wiring and the current negative liquidation result, not profitability.

Validation:90 targeted cases cover actual paper/native threshold comparisons,
zero-fee FIFO, production restart/cooldown, persisted loss exits, expired
research, missing depth, authoritative holding time, unknown acceptance,
external holdings, original settlement behavior and collector lifecycle.
Synthetic executions establish correctness only. The separate native build
identity and preserved old binary hash are recorded in
`data/receipt-native-build-20261005.json`.

The collector's12:28 shutdown was graceful with no saved STOP/error provenance;
its cause is unresolved. The collector now records signal/marker/status-failure
causes independently and preserves status-publication failures as failures.
Recovery and deployment reused the existing services and left activation and
accounting unchanged. A later unchanged healthy process should not be restarted
to repeat this work.
