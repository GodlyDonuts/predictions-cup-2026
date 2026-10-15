# Offline queue-state amplification audit

Task [#129](https://github.com/GodlyDonuts/predictions-cup-2026/issues/129). This research package includes the unchanged v1 queue core in a private Rust module. It opens only newly created synthetic fixture directories, links no transport/credential code, and performs zero account calls. No Store, SDK, native strategy or live worker is modified.

The checked ARM64 run used `nice -n 10`, a separate directory, at most10,000 bindings for clone/hash/serialization/fsync samples, and synthetic journals with unique deterministic payload hashes. Its only larger case generates a100,000-binding compression fixture; it does not collect additional CPU sample points. The raw fixtures remain private, including the unvalidated output of a timed-out xz case; only sanitized aggregate measurements are published.

```text
cargo test --offline --locked --manifest-path research/queue-state-amplification-20261015/Cargo.toml
cargo build --offline --locked --release --manifest-path research/queue-state-amplification-20261015/Cargo.toml
nice -n 10 target/release/moloch-queue-state-benchmark shape.json /ABS/NEW_BENCH_DIR/benchmark.json
python3 -m unittest discover -s research/queue-state-amplification-20261015 -p test_analyze.py
nice -n 10 python3 analyze.py /ABS/NEW_BENCH_DIR/benchmark.json /ABS/NEW_BENCH_DIR/analysis.json --compress
```

The binary is under this package's `target/release`. Output must be a new absolute filename in an existing benchmark directory. Shape bounds cap the workloads; all original synthetic data is retained or only the benchmark's verified private temporary directory is cleaned. The compression script requires the exact bounded fixture filename/hash, creates new outputs, checks full decoded bytes/SHA256 and never deletes sources.

`analyze.py` separates the requested100-read offered-load stress cases from the current serialized completion-pacing upper bounds. Logical endpoint caps do not imply achievable combined throughput. The size model is exact for fixed synthetic identity lengths after the first entry; empty maps omit two commas. It excludes status/wait records, active/rate overhead, longer identities/sequences and retained checkpoints, so it is not an exact production forecast. Projections past a single v1 record's64MiB cap are explicitly counterfactual. Compute-work measurements are wall times, including live-host scheduling, rather than process CPU measurements.

See [the report](../../docs/MOLOCH_QUEUE_SCALABILITY_20261015.md) for measured results, archive protection rules and the proposed migration. Implementation or removal of any production history requires the parent's explicit approval.
