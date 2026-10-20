# Original SIG capture semantics — October 20

The completed, pre-independent-test source fixture now passes semantic decoding and the real offline delivery engine. This closes a source-format gap; it does **not** establish continuous coverage, execution calibration, training admission or model profit.

## Original records and provenance

The fixture is completed capture segment 0, October 20 approximately 20:15–20:25 UTC. The existing Node audit verified all 15,088 original producer hash links and preserved those hashes when producing canonical transport bytes. Original file hashes, canonical bytes and terminal producer hash were checked again before decoding. REST records retain the SHA-256 of their original line under a pinned whole-file manifest.

The original session manifest was recovered separately and matched SHA-256 `006c8e318c582d5277efc7338d0a1cd33b46c011e3a169a980dea6c7a208b191`; its tournament matches the verified broadcasts/REST receipts, and token persistence is false. Original subscription acknowledgments omit the tournament ID. The decoder therefore requires an explicit pinned session binding for those acknowledgments rather than pretending the field was present.

| Decoded source | Observations |
|---|---:|
| WebSocket book snapshots | 14,197 |
| REST book snapshots | 401 |
| Original subscription gaps | 237 |
| Unique official trade prints | 272 |
| Observed exchanges | 237 |

Sixteen older WebSocket versions were ignored under the existing per-exchange version rules. Both channels share version-conflict checks. Original transport resync flags and subscription invalidations remain explicit. Same-millisecond ties use REST-before-WebSocket, with original order retained inside each channel; this is a declared ordering convention, not submillisecond truth.

See [semantic result](prospective-semantic-fixture-result-20261020.json).

## Distinct clocks and delivery

Source `014d6dd6`, subscription binding follow-up `bec25f4a`, and REST-gap follow-up `0e83857b` add an explicit original REST snapshot origin. The actual receipt time and exchange `asOf` time remain separate. No native processing clock is fabricated, and a later receipt does not make an old exchange version fresh.

REST snapshots and REST-discovered gaps are **not** automatically broadcast to a connected simulated actor. The actor receives their information through charged reads. A replay regression checks that an empty REST context changes hidden-world validity while leaving the previously received actor cache untouched until another paid observation.

The source tests exposed a real precision defect in `int(datetime.timestamp()*1000)`: `1970-01-01T00:00:01.001Z` could become 1000 ms, accepting a one-millisecond future source/request. Timestamp conversion now uses exact UTC integer arithmetic and floors submillisecond precision. Existing timezone requirements remain. Frozen datasets and the active learner were not rewritten.

The 73-check source/adapter/replay suite passed; the final REST-gap change also passed its 33 relevant checks.

## Actual engine integration

The normalized original observations and prints were packaged into a separate research cache and consumed by `ApiTape`, snapshot-world replay, derived real-time delivery and the paid data service. The terminal boundary excludes extrapolation past the last original receipt.

- All 237 hidden-world exchanges and 237 actor book caches were represented.
- The engine delivered 14,197 original WebSocket books, charged 995 managed SIG read admissions and one simulated token mint admission, and processed 272 original flow facts.
- The 598,541 ms source interval advanced in 3.675 seconds. **No model inference or training was included**, so this is not a training throughput measurement.
- The harness submitted no contest order; synthetic cash was unchanged and there were no fills. These are source-integration invariants, not a policy return.

See [runtime integration](prospective-runtime-integration-20261020.json). The actual model-fit source remains frozen population 029, with its previously verified 33-update checkpoint. Its two-day nominal/stressed selection is still pending.

## Capture continuity and storage

At 23:45:50 UTC, controller 949555 and reader 965117 were verified present, with segment 20 capturing and 20 completed segments. Free space had recovered to 19,149,873,152 bytes through the already-configured lossless archival mechanism. Another 111,336,988,451 bytes in strictly superseded SDK generations were eligible for that mechanism, excluding the selected generation. No new collector or storage control was installed.

This is capacity evidence, not a guarantee of four-day uptime. Continue checking actual process handles, completed segments and disk headroom. Financial STOP/control hashes matched their original values. Independent October 21–25 features remain unopened; no trading or financial state changed.

Remaining work includes continued-segment compilation, source coverage/lifecycle qualification, development model selection/evolution and the later reserved independent test. A successful fixture does not complete those requirements.
