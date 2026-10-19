# Per-market and global recurrent controller

The previous v2 policy encodes every current contract, averages those embeddings, and feeds one global GRU. It can retain aggregate history but has no dedicated temporal state for a particular market.

The v3 candidate adds one64-unit recurrent state per observed market before pooling, then retains the existing1024-unit global controller. Identical current quotes can therefore retain different market histories. New/absent market slots start from zero; actual presence and asynchronous episode resets are preserved. The packed actor state explicitly includes market memory and presence, rather than silently changing a legacy checkpoint shape. Checkpoint provenance records the architecture and every dimension.

CPU checks verify that sequential actor inference agrees with fused training sequences under new markets, disappearing slots and asynchronous resets; changing a future observation cannot alter a prior output. Gradients reach the market memory. Exact monetary accounting, external action masks, simulator mechanics and execution authority are unchanged.

This is a separate candidate. The existing128-block v2 run continues with its original frozen source and objective. V3 GPU throughput, memory use, development performance and held-out performance are not yet verified. More recurrent state is not evidence of trading skill, calibrated fills or profitability. No learned policy is deployed.
