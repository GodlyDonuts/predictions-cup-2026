# Sealed trace replay contract, version 1

Frozen before validation of any actual trace cohort. Inputs are a separately
pinned manifest (CLI requires its raw SHA256), immutable sealed trace/sample
gzip files, terminal seal, original producer and mapping bytes, and separately
retained original SIG segments. The manifest fields are:

```
version: 1
seal, trace, samples, producer, mapping: {path: ABSOLUTE_LOCAL_COPY, sha256, bytes}
sigSources: [{recordedPath, recordedDevice, recordedInode, path, sha256, bytes}]
```

Local paths may point to detached copies. `recordedPath/device/inode` identify
original bookmarks, not the detached copies' filesystem identity. Their source
provenance is an externally pinned input, never self-authentication. All local
inputs must be single-linked regular files, not symlinks; their raw hashes,
lengths and stable identity are rechecked. The seal's original private paths are
not followed. No code path comes from the manifest. Only the repository's exact
reviewed observer is loaded after an allowlisted raw source hash check (the
reviewed LF and CRLF variants). Its Study is instantiated with the exact
run-start configuration, with no network or producer execution.

Trace and samples are read in bounded streaming gzip mode: compressed total
including seal <=2GiB; per-line <=32MiB; total decoded bytes per file <=32GiB;
records <=10million. Each SIG reference reads only its bounded original line at
its exact byte offset. Cache body decode <=16MiB. Prior cache bodies are released
following apply completion. JSON duplicate keys, incomplete lines, bad gzip,
changed files or chain/hash conflicts reject verification.

A strict event state machine requires one run-start, original policy/config and
mapping binding, sequential original cache route calls, each successful full
body before apply, exact sampler now arguments, all begin/completion pairs,
output ordinal ranges and terminal closure. Transport failures are recorded
failures, not synthetic bodies or no-trade zeros. Each consumed SIG row is
checked against the separately pinned raw segment, exact newline/hash/sequence,
bookmark and original row-chain bytes. First initial EOF/partial-line behavior
and subsequent consumed byte offsets are checked. Rotations keep independent
segment provenance. Any unread old-segment bytes or unconsumed buffered final
rows are explicit coverage limitations; they are never promoted to global
continuity. A later seen SIG row is not moved earlier in the invocation order.

Every genuine Study callback is replayed at its exact original supplied now.
Every emitted sample/label is compared, in order, with the sealed original
sample file. No-sample callbacks are still replayed. Extra/missing outputs,
changed triggers, changed original clocks, altered label book choice, pending
input, unsupported event, malformed body and source conflict fail. Successful
replay proves exact known-sampler inputs/consumption/emissions for this suffix;
it does not prove provider trade completeness, collector processing or epoch,
full tournament topic bindings, settlement equivalence, calibrated probability,
economic profit, full history or financial authority. Those fields remain
unavailable/false. No active cohort/outcome data is read by development tests.

## Running and checked scope

```
python trace_replay.py ABSOLUTE_MANIFEST --manifest-sha256 ORIGINAL_MANIFEST_SHA256
python test_trace_replay.py -q
```

The source allowlist is the independently reviewed input-trace producer
`69f7bbc3`, raw LF `200abfd0701f56f43183b1b7b60cd1cdf11525f0fb9daea72e9bf6f08d90bc40`
and original CRLF `1f63cf06d05b68e46d2febe57607125d9eb95d6e87db3f4d29b401d39d8e700b`.
The verifier compiles only those checked bytes from the fixed repository source,
not any manifest-selected file or cached Python bytecode. It performs no HTTP,
producer main invocation, data acquisition, financial mutation or activation.

Eighteen synthetic original-shaped checks pass locally. They include actual
book and trade callbacks, duplicate/no-sample polls, genuine failed polls,
initial partial source lines, unread independent rotations, sealed raw source
membership, exact sampler arguments/emission ranges, altered outputs and
clocks, replacing the first horizon book with a later book, malformed pins,
missing source, pending operations, body conflicts, reversed routes,
decode bounds and refusal of manifest-selected unreviewed code. No actual
cohort results were inspected. `firstConsumedBookReplayVerified` is unavailable
when there are no horizon labels, rather than claiming a first-book proof from
a sample-only cohort. All success fields refer to these pinned recorded inputs,
not independently attested completeness of unrecorded requests. A fully
recomputed fraudulent capture cannot self-authenticate its execution merely by
supplying matching hashes; external source custody and the reviewed producer
remain explicit trust inputs.
