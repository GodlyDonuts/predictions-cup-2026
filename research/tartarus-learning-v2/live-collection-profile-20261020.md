# Live collection profile and exact-byte hashing candidate

An exact creation-verified handle identified GPU020trainer11404 before a15-second,20Hz `py-spy0.4.2` sample. Sampling used `--nonblocking`, no native/GPU profiling, no locals and no subprocess following. It made no model/source/clock/collector/account change. The learner remained live and advanced from158,107to158,619recorded decisions during the sample; both endpoint events were closed-episode collection. Py-spy reported296samples and three failed stack reads. Nonblocking samples can be imprecise.

[Aggregated stacks, process metadata and raw-profile hash](gpu020-cpu-stack-001.json).

| Sampled main-thread location | Fraction | Interpretation |
| --- | ---: | --- |
| `ApiAffordanceEpisode.observe`, inclusive |43.58%| Includes its parent observation builders; do not add nested inclusive percentages |
| `EpisodeWriter.record`, inclusive |20.27%| Recorder work, including actual input-tensor hashing |
| digest `walk`, self |17.57%| Includes device-to-host tensor-copy/hash dwell |
| consolidated categorical validation, self |12.84%| Includes device synchronization; not necessarily CPU arithmetic |
| packed input transfer, self |10.47%| Host/device staging and transfer dwell |

These are short live wall-stack proportions, not CPU utilization, kernel timing, isolated benchmark speedups or proof of GPU saturation. They motivate reducing repeated data handling before increasing network complexity. Existing background external collectors continued during the sample.

## Candidate: one actual-byte transfer per recorded frame

`episode_observation_digest=packed_bytes_v1` opts the sealed episode writer into packing that frame's actual post-forward input-tensor bytes on their existing device, transferring one contiguous byte buffer to CPU, then hashing the original canonical dictionary keys, dtype/shape headers and byte slices in the unchanged order. It does not hash a cached source feature vector or replace validation with a source assertion. Extra packed storage is bounded to one frame, not an entire rollout.

The default remains `legacy`. The independent reconstruction reader still uses the legacy hashing implementation, so a wrong packed digest fails reconstruction. There is no change to prices, masks, legal actions, actor RNG, virtual time, rewards or original source files. No old frozen trainer or queued source was edited.

Twenty-two focused CPU tests passed in5.178seconds, with one CUDA-only test skipped. The full action suite passed384checks in70.556seconds, with five CUDA-only checks skipped. Canonical byte parity covers nested key ordering, noncontiguous tensors, scalar/empty tensors, integer extrema, half/double precision, signed zero and explicit NaN bit patterns. Mutation changes the digest. A packed-writer fixture reconstructs and trains through the independent legacy reader; a deliberately bad packed digest is rejected.

GPU byte conformance, extra-memory capacity and wall-clock benefit remain unmeasured. This candidate is published for a future serial validation point and is **not enabled in the current trainer or queued GPU021profile**. No total-training speedup or economic improvement is claimed. The next committed experiment remains the already-checked [public-trade observation profile](public-trade-stream-20261019.md). Full chronology, calibrated execution/valuation and independent positive ROI remain incomplete.
