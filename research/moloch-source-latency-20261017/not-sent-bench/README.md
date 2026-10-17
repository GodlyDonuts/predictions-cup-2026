# Original not-sent verification microbenchmark

This standalone Linux Rust harness measures the normal never-attempted branch
of `fundamental_policy::Cohorts::original_not_sent_records` (lines 730–752 at
`df132d7a`). It does not open a Session, SDK queue, account client or service.
It implements no cache and changes no authority, clocks or financial state.

Each timed invocation keeps the original work: held dispatch descriptor;
new descriptor/path/identity pin; full initial read and SHA; another full read;
every row's JSON parse, clone, outer-hash removal, canonical serialization and
SHA; consecutive sequence/previous checks; exact selected not-sent proofs;
final full pin read/SHA and metadata/descriptor checks. The file bound stays
64 MiB and row bound 2 MiB. The descriptor's expected head comes from the
separately pinned original input, rather than loading an actual DispatchLog.
The SDK-proved-abort branch is unsupported, never approximated by a Boolean.

Before timing, it verifies both original file hashes and full dispatch and
fundamental chains. A limited sidecar projection independently derives all
current retired-top-up not-sent records, including cohort removal. It refuses
unsupported events or replacement history. Those exact original rows must
equal the dispatch selection and independently pinned expected JSON/digest.
This is not a complete Rust cohort or native financial replay.

Use the **same arbitrary-precision `serde_json` and `sha2` rlibs** from the
checked runtime's target. Select exact dependency files, rather than the first
wildcard match if multiple versions exist. The numeric sentinel fails closed
if the wrong `serde_json` features are linked. No Cargo changes are needed:

```sh
rustc --edition=2024 -O bench.rs \
  -L dependency="$RUNTIME_TARGET/debug/deps" \
  --extern serde_json="$EXACT_SERDE_JSON_RLIB" \
  --extern sha2="$EXACT_SHA2_RLIB" -o not-sent-bench
nice -n 19 ./not-sent-bench /PRIVATE_COPY/input.json EXPECTED_INPUT_SHA256 \
  > /PRIVATE_COPY/result.json
```

`input.json` schema 1 pins `dispatch` and `fundamental` basename, byte count,
raw SHA, final sequence and previous hash, plus `expectedNotSent` keyed by
native qid and its canonical SHA. Raw originals and the input packet remain
private. Supply the separately reviewed input SHA on the command line.

There are two unreported warmups and seven timed invocations. The aggregate
output contains per-round/component milliseconds, input/source hashes and
native qids; it contains no original venue order identifiers or raw rows.
Independent expected-output comparisons occur outside timing. Median
components need not sum to the median total. A seven-call entry-path estimate
would be arithmetic, not a measured production latency or proof of causality.
No timing, speedup or production result is claimed by this source handoff.

## Actual original-work measurement

Root compiled and ran the frozen helper on the ARM64 VPS using the exact runtime dependency archives. The first link omitted thin LTO and failed; its original log remains retained. Adding `-C lto=thin -C codegen-units=1` matches those release archives; the corrected compile and run both exited zero. No benchmark source or input was changed. Three independent candidate targets were compiling concurrently on the six-core host, so this is a component observation under that load.

[Root-verified aggregate](aggregate.json) authenticates fifteen proof members, the original frozen helper and input, seven measured rounds after two warmups, and every reported median. The original 21,894-row / 10,652,764-byte dispatch and four retired-top-up selections are verified by the helper before timing.

| Component | Median |
|---|---:|
| Initial held-file pin/read/hash |69.907ms|
| Additional parser input read |4.318ms|
| Whole-chain parse, clone, canonical digest and selection |267.981ms|
| Final full pin/hash and descriptor checks |70.088ms|
| Total invocation |412.409ms|

Seven calls multiply to2,886.862ms arithmetically; that is **not** a measured worker phase or causal attribution of the observed book delay. No cache speedup is established. The separate candidate will retain a full file hash on every cache hit and reevaluate original path/version/current descriptor and attempted-order predicates; only repeated parsing may be reused. SDK-abort archives stay on their original full verification path. No account requests, Session replay, production changes or profit follow from this measurement.
