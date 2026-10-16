# Original input and consumption trace, version 1

This protocol is frozen for a **new opt-in read-only cohort**. It does not
reinterpret sealed v7 or trade-v1 results. The existing book sampler and
`031edcb2...` trade sampling policy, ages, horizons and future-label rules remain
unchanged. Trace mode adds synchronous local capture I/O; its latency and
selection effects are a new producer/capture condition, not interchangeable
with an untraced cohort. No outcomes from the active cohort were inspected.

Run the reviewed producer with its existing arguments plus `--input-trace
--trace-mapping ABSOLUTE_ORIGINAL_UNIVERSE_JSON`, in a newly chosen output
directory. The operator must retain the original SIG segments and exact
producer/mapping files independently. A mapping file hash alone does not certify
the effective cache mappings or contract equivalence: each original cache body
is the witness of the actual mappings supplied on that call.

`input-trace-v1.jsonl.gz` is a sequence/hash chain of concatenated gzip members.
Every member is fsynced. A run-start record pins the installed producer bytes,
effective sampler configuration, policy, original mapping file and initial
bookmark. Each original cache response is retained as base64 bytes with its
byte SHA256, route, request/read completion clocks and status **before JSON
parsing**. Every successful call, including no-sample calls, has an apply-begin
with the exact sampler `now` argument and an apply-complete with output ordinal
range and actual completion clock. Original source/receipt/export timestamps
inside the body are never replaced. Failed polls are retained without error
text or invented response bodies. If opening/reading raises, unavailable body
bytes remain unavailable. An oversized read retains an explicitly incomplete
16MiB prefix and stops the capture; that prefix is never called a full body.

Each consumed valid SIG row pins the raw line SHA256 **including its original
newline**, byte offset/length, sequence/hash/previous, actual segment identity
and bookmark. No reserialized row is called original source. Its original event
type also identifies consumed resets. The actual sampler invocation and apply
completion are recorded around the unchanged callback. Initial EOF/partial-line
bookmarks explicitly exclude the unread prefix. Observed rotation records pin
old/new bookmarks without claiming cross-segment chain continuity. If a reader
moves with unread bytes, those offsets/sizes remain visible; instrumentation
does not silently repair the reader or fabricate missing consumption.

`trace-samples-v1.jsonl.gz` contains the sampler's unwrapped original row bytes
(one JSON line per gzip member). Default mode still uses the original Output
implementation. Trace mode never calls its pruning path. The shared compressed
limit for both trace/sample files is **2GiB**, including a reserved **64KiB** for
the terminal seal, and each append requires **10GiB** free after its projected
write/reserve. Compressed length is known before writing. Files are create-new,
mode0600, and no data is deleted to recover capacity. Write, sync, space, limit
or file-identity failures poison the trace and exit only the sidecar; they
cannot be swallowed by the cache-transport exception handler. Status records
capture failure when writable. A failed append may leave a partial tail; it is
retained for diagnosis, never repaired or called complete.

The create-new `input-trace-seal.json` pins the actual final compressed file
hashes/lengths, trace head, output count, producer/mapping references and final
bookmark. It is complete only after normal stop, no pending apply/poll, no
poison, and matching bytes/reference hashes. A trace with failed HTTP polls can
have a complete *capture* seal because those failures were genuinely recorded;
that does not claim successful inputs or economic coverage. Missing/incomplete
seal denies complete witness use. A crash between any writes leaves no valid
terminal completeness claim. The seal is bounded, fsynced and directory-synced
on the Unix deployment filesystem.

Capture alone sets `wholeWitnessVerified:false`. Independent verification must
check the retained original SIG bytes, every raw cache body, source identity
conflicts, sampler replay using each exact recorded argument/order, emission
membership, first **consumed** eligible future book, and missingness. It must
not infer collector processing/epoch attestations, financial authority, global
trade completeness or settlement equivalence. The existing witness verifier
has not been broadened to assert those guarantees merely because a trace
exists. No extra account/exchange requests, clock renewal or financial-service
changes are introduced.

Validation: 16 new trace tests and 78 existing observer/economics/toxicity/trade
sampler tests passed locally (94 distinct checks). Tests include original raw
body membership and argument timing, no-sample/failed polls, actual trade
trigger without future label, original SIG offsets/partial initial line,
byte-identical default golden sample output, hash-chain/seal membership,
partial writes/fsync failure, changed mapping/same-size tampering, limits/free
space and create-new cohort refusal. Deployment and new-cohort operation remain
with the parent execution owner.

Deployment byte pins: the author's Windows CRLF source/protocol hashes are
1f63cf06/0ae3b843. Root verified newline-only equivalence to Git-LF source
`200abfd0701f56f43183b1b7b60cd1cdf11525f0fb9daea72e9bf6f08d90bc40`
and Git-LF protocol
`0192b4ab31f5f278cdc1f5ae341e17234c1f12a9dc0b3af43e742071314bc853`.
Actual deployment must pin its real bytes; these hashes are not interchangeable.
