# Exact CUDA observation copy qualification — October 21

Checked source `9f1d6ad5` / `bd51a7ec` constructs large actor input batches directly in one owned CPU allocation per dtype, then uploads those allocations without another concatenation. Every original entity, feature, mask, padding byte, dtype and shape remains. CPU replay and smaller batches retain the original padding/concatenation algorithm. Allocation reuse requires an explicit call and independently verifies complete contiguous storage and offsets. It is not an entity, action, decision or API limit.

Thirteen focused checks pass, including the actual historical recurrent policy's CUDA outputs, sampled action values and parameter gradients. Original source arrays can change after assembly without altering the owned output. Noncontiguous inputs, promoted dtypes, empty rows, signed zeros and NaNs retain their original bytes.

The actual original population030 cohort was reconstructed from its sealed action tape: **6,126 decisions, eight worlds and107 batches**. Every original observation, clock, reward and financial-state hash passed. All **1,699 input frames** also passed an independent CPU-versus-actual-CUDA byte comparison, exercising the fused path every time. Peak probe CUDA allocation was67,108,864bytes. [Exact completed proof](packed-observation-cohort-parity-20261021.json).

The first probe tried retaining whole chunks on CUDA under a12% per-process memory cap and reached that cap. Its artifacts remain. The successful probe retained chunks on CPU and checked/released one CUDA frame at a time. The active learner was not stopped or restarted by either probe. The successful reconstruction took41.83seconds preparation and318.74seconds replay including the additional per-frame CUDA checks; this is not a matched end-to-end training speed comparison.

Synthetic alternating five-trial measurements on the occupied laptop,12 iterations/trial, measured:

| Workload | CPU assembly ratio | CUDA assembly/upload ratio |
|---|---:|---:|
| Eight worlds,1,200–2,000 entities | 0.99× | 1.41× |
| Five worlds,1,400–2,000 entities | 0.94× | 1.19× |
| Two small worlds,20/64 entities | 0.93× | 0.93× |

Ratios compare original time with candidate time. The fallback algorithm is unchanged, but this occupied-host sample does not establish zero overhead or a speed improvement for every case. Large-copy results are component measurements only. [All timings and exact input hashes](packed-observation-benchmark-20261021.json).

The active032 training run stays on frozen `b83d8c87`; it has **not received this optimization**. Its20-second CPU sample contained997 samples with no sampling errors, showing observation construction, tensor transfer and trajectory hashing alongside source replay work. [Profile summary](population-032-cpu-profile-20261021.json). A future source transition must retain existing results and verify the actual training boundary; this qualification does not authorize discarding ongoing full-day trajectories.
