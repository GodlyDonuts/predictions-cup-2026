# File-backed replay without changing the trading problem

The optional `replay_cache` path replaces whole-corpus Python dictionaries and prepared-book maps with a source-pinned, read-only SQLite replay. Original compressed sources, both manifests, cache bytes and embedded source claims are checked. The original dataset SHA, calendars, observation protocol and provenance remain authoritative. A cache cannot turn partial history into a full-history or independent-test claim.

The loader decodes records on demand. Four chronological source cursors stage world snapshots, information arrivals, public prints and counterparty execution; their tie order is preserved before expiry, arrival and receipt events. There is no source-event truncation, decision throttle or artificial one-million-event WAIT abort. Same-role initial state is reconstructed through indexed last-state and first-discovery queries, preserving read-target order, gaps, source ages and confidence-expiry counts. Prices still require paid observations. The512-book LRU bounds a decoding cache and never removes market depth or choices. Trade-history cursors query the immutable tape without storing its full past per world; their original snapshot and processed-row watermark prevent future or unprocessed same-clock prints entering a paid page.

The actual875,744-row source and4,202 prints produced a1,153,351,680-byte cache. A finite real-source probe opened and verified it in0.940seconds, built the role index in1.065seconds, and reconstructed one hour of prior state in0.341seconds. It restored237 hidden books from86,990 original world rows while decoding1,918 records, with zero actor books/references and four queued source cursors. Peak process working set was633,221,120bytes; private committed memory was1,791,664,128bytes. These include Torch and a small materialized comparison range, but no58M GPU model. They are not an apples-to-apples speedup ratio against the cancelled full-model CPU audit.

Thirteen actual first-minute checkpoints matched eager replay exactly for observations, masks, entities, cash, orders, fills, realized accounting and clocks. The probe includes a simulated one-share fill solely to exercise accounting; it is not a policy sizing rule or contest transaction. [Real-source result](disk-replay-real-source-probe-001.json) / [local reproduction script](disk_replay_probe.py).

A separate eight-world/16-step CPU probe using a234,039-parameter engineering model accepted fused PPO, detected optimizer mutation, matched serial/batched financial state and recurrent values, and shut down cleanly. Its update took1.375seconds. Batched collection was slower on this small CPU model (serial/batched0.834); no general compute speedup is claimed. All97 runtime source pins matched the checked files at measurement. This is not the58,157,799-parameter GPU capacity qualification. [CPU proof](disk-eight-world-cpu-proof-001.json).

185 action-suite and12 realism checks pass. They include source/cache tampering, source-claim relabeling, double calendar fences, unprocessed same-clock prints, late reports, counterparty tie order, exact prefix discovery/expiry state, raw/full-depth stochastic decisions, fused log probabilities, actual full-history/independent-test rejection and a finite1,000,002-event WAIT. Full-size GPU capacity and live population integration are still required before reporting this path as active training. Frozen015 and the existing queued016 have not been hot-loaded.

Build a new private cache, preserving the source dataset:

```powershell
python -m moloch.learning_actions.disk_tape --dataset SOURCE --output NEW_CACHE
```

Use its returned manifest SHA in an experiment:

```json
"replay_cache": {"path": "PRIVATE_CACHE", "manifest_sha256": "RETURNED_SHA256"}
```

Keep `dataset` pointing at the original verified dataset. The preparation CLI also accepts `--replay-cache` plus `--replay-cache-manifest-sha256`; the original chronology, coverage, mechanics and held-out provenance checks remain enforced. Cache creation, source replay and all measurements issue zero account requests and change no production financial state.

This removes the measured whole-dataset memory bottleneck. It does not supply missing competition history, calibrate fills or official valuation, finish relationship/collateral/lifecycle parity, or establish independent profitable ROI. Long-lived simulated order/fill accounting still consumes memory; this is bounded source staging, not a promise of constant total memory for every possible policy.
