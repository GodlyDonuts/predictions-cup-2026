# Executable fill-quality measurement

The diagnostic matches an attributed purchase to its exact original source sequence/hash/version and independently requested subsequent depth receipts. It verifies the original wallet chain/accounting and complete public-source chain. It does not create sales or change active bank/fee/fill assumptions.

For the first Kansas portfolio purchase at00:24:53.968UTC October6, paid price0.355 came from raw ask0.350 plus a modeled0.005 adverse entry tick. The entry bid was0.345. Later independent depth receipts at5 seconds,1 minute and5 minutes still showed bid0.345. Thus the−0.015 estimated liquidation comprises:

| Component | Per-share contribution |
| --- | ---: |
| Observed entry spread |−0.005|
| Modeled entry adverse tick |−0.005|
| Modeled exit adverse tick |−0.005|
| Subsequent observed bid change |0.000|
| Fees |0.000|

Raw-receipt quote roundtrip is−0.005; the conservative tick-stressed estimate is−0.015. Neither is an accepted sale or realized P&L. Both latency cases are the same opportunity and are deduplicated. One observation does not establish favorable or unfavorable selection for the strategy. These early receipts show a spread/friction problem, not adverse market movement after this particular purchase.

Protective limit cushions and realized/counterfactual fill prices must be distinguished. Future exact-receipt baselines can retain protective limits while reporting actual later levels separately from additional residual stress. Preserve current frozen fills and original accounting; never remove stress or rewrite a losing wallet to claim profit. Even the raw baseline remained negative here.

Missing or too-late receipts, immature horizons and insufficient eligible depth are explicitly censored. No mark is forward-filled. Current defaults use5s/60s/300s horizons, at most20s capture lateness and one-percent displayed depth. Four identity, decomposition, clock and censoring checks pass. Full results/source hashes are in data/portfolio-fill-markouts-20261006.json.
