# CUDA neural paper cohort

`gpu-dataset.mjs` extracts the existing delayed FIFO round-trip labels from a verified journal, using isolated market histories and zero additional explicit fees. It purges training labels crossing the chronological development boundary, retains censored/no-fill rejection counts, and requires the existing supported-instrument coverage floors. Its restricted-runtime dataset excludes journal account payloads.

`gpu-train.py` fits 128 configurations of a small ReLU network, batching independent seeds and regularization strengths on CUDA. Normalization and target scaling use training rows only. Development RMSE selects three models per eligible horizon. This is model selection on a short correlated capture, not proof of profitable performance. Insufficient horizons are rejected rather than relaxing support.

Publish a frozen bank only after validating its exact source binding, normalization, chronology and all exported inference vectors. The JSON weights execute in the existing JavaScript paper engine without a Python inference service or extra market requests. `predictMLP` checks shape and finite values before forecasting. Future wallets use the unchanged depth, cost, latency and exposure controls. The new bank explicitly binds `revoke-on-invalidation-v2`: missing/invalid books and account outages revoke pending entry forecasts; existing frozen banks retain their historical lifecycle.

Local commands (runtime paths supplied by operator):

```text
node live-paper/gpu-dataset.mjs --journal <journal> --config <zero-fee-config> --out <dataset>
python live-paper/gpu-train.py --dataset <dataset> --out <candidate-bank>
```

Use the CUDA build from the [official PyTorch installer](https://pytorch.org/get-started/locally/). The first RTX 3060 Ti cohort trained 64 of 128 candidates in 14 seconds; remaining candidates lacked matured supported labels. Its 64 GPU/JavaScript parity checks had maximum absolute error 1.05e-17. This validates implementation, not trading edge.

The dashboard displays worker heartbeat freshness separately from accepted market-book freshness. Its ML table defaults to zero-fee wallets; legacy fee-stress results remain in the API for reproducibility. No contest orders or credential creation are part of this pipeline.
