**Correction, October21 04:56UTC:** V2 incorrectly preserved book baselines after healthy token refresh. Official SIG documentation requires a REST refetch. Its queued034 handoff was cancelled before activation; active033/production never used it. [Correction and V3 status](refresh-protocol-correction-20261021.md). The dated results below remain historical evidence.

# Tartarus ML: full decoder parity and actor timing — October 21, 04:08 UTC

The previous goal turn made concrete progress through the optional replay parser, later real checkpoint verification and completed founder comparison. This continuation completed the exhaustive decoder qualification and corrected missing actor-preparation time. The full learning objective remains incomplete; independently demonstrated positive ROI is absent. No production financial state changed in this ML continuation.

## Exhaustive replay result

The private checked59d13e1d audit completed successfully: all15090226 market records and79847 trade records match the default parser in exact Python types and values, including IEEE754 float bits. Both source database hashes were checked; database99417776, cache manifestdf6d11e9 and auditbee3a24d are retained in the [whole-cache result](full-cache-decoder-parity-20261021.json). The equality pass took595.900s. This is a parse-equivalence result, not training speed or profitability.

The optional decoder is now eligible for an explicitly configured future reader on this exact cache and matching implementation. It requires the audit path plus pinned audit hash. Its proof binds orjson3.11.3, the actual Windows extension and Python files, and Python3.12.14. A different platform, module build, cache or unpinned/partial audit rejects. Default readers remain stdlib; frozen033 and the existing CPU comparison are unchanged. The earlier40000-row parse-only sample was about2.5x faster; no whole-training speedup is measured or claimed.

## Actor timing correction

Old calibration constructed observations and transferred tensors before its timed interval, and never measured converting the result into a host action. Checked02a3c953 times observation construction, tensor preparation/transfer, stochastic action-only inference and host conversion, including device synchronization. It preserves policy RNG, actual weights, wallet and virtual time. Training-only critic/entropy statistics are excluded from the default live-action profile; full_statistics is an explicit alternative.35 focused tests pass with zero skips. [Exact tests](complete-actor-calibration-checks-20261021.json).

A separate actual-source CPU study used the immutable316-update,5272685-parameter checkpoint and the original frozen fit calendar/configuration, with explicit legacy refresh behavior. Startup had2798 entities and a23ms measured local p95 ceiling. After one simulated minute with99 budgeted data-read admissions,2928 entities measured27ms. There were no counterfactual fills, optimizer updates or physical requests. Weights, wallet, world clock and RNG matched before/after both measurements. [Actual source and checkpoint result](complete-actor-calibration-real-source-20261021.json).

These are current-shape, single-thread local CPU measurements on an occupied laptop. They are neither GPU/VPS measurements nor a maximum-context guarantee. The existing033 experiment and its CPU comparison retain their original frozen12ms GPU compute cost; comparing23/27ms CPU against12ms GPU does not isolate the code change. No score from the old experiment was silently reinterpreted.

## Learning continuity and remaining requirements

Actual7108 remains the same CUDA learner with its original source/config. Its latest observed progress has418 continuing optimizer updates,19340 decisions, five completed full48h worlds out of eight and422 simulated fills. The original founder's completed48h marked ROI remains negative13.59% nominal/negative10.35% on a separate stressed path. The immutable80-update candidate CPU comparison is still processing the complete second case; no trained-versus-founder outcome is established yet.

The extended staging cache is complete but not yet used by033. Full original competition-history book coverage, historical mapping/publication-time qualification, the requested full-history/four-day evolution/four-day independent protocol and counterfactual/passive execution calibration remain incomplete. The protected October21–25 test is unfinished and unconsumed. Passing engineering checks, finite trained weights and simulated fills do not prove independently profitable behavior. Current processes are left running; no generation is discarded to adopt an optimization prematurely.

## Exact optional reader configuration

The reader requires the inner [admission artifact](replay-decoder-admission-20261021.json), SHA256 bee3a24d28a9959556bb8695eab5c58161af27439d86901d675eb822df0d20ab, rather than the outer reporting wrapper. Set replay_cache.decoder_audit to that file and replay_cache.decoder_audit_sha256 to that hash, with cache manifestdf6d11e9 and the same installed implementation. This artifact does not match the original active033 cachef066e68b. A separate exhaustive parent-cache parity audit is queued/running to enable a future parsing change without changing historical inputs;033 remains unchanged.
