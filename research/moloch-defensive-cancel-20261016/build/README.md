# Actual ARM64 build, source b112c6c1

Candidate only; not production deployment evidence. Final checks passed on the VPS: SDK231, runtime library197 and worker95. These are distinct suites; the new14 SDK,13 resting-assessment,2 dispatch,6 worker and7 helper checks are subsets, not totals to add again. The original 94-pass/1-fail worker receipt-directory fixture and test-only library Receipt Debug compile failure are retained; both corrections affect tests only.

Final checked run16:50:12-16:57:53 UTC took461.269 seconds, including308.831 seconds for the optimized release. Binary SHA256 `3ab6ef7c49c089f19ff90220cd3431ff5ce1c84a5858d6d9fdde060f1b9d6883`,13,088,680 bytes. All251 raw inputs remained pinned; nine changed/added paths from the248-input predecessor include no native financial source change.

A tiny offline probe linked the exact native rlib identified by the final Cargo compiler-artifact metadata and printed engine identity `8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`. This matches the existing live engine. Its original source, executable hash, rlib hash and runtime-binary binding are retained in engine-identity-evidence.json. The initial probe link omitted the release thin-LTO flag and failed; the successful probe used the same original release rlib with thin LTO, changing no runtime source or binary. Neither probe makes any account request.

The deployment candidate enables defensiveCancel and selects the checked binary only. Banks, original seed/contributions, native engine, twelve bridges, fees, risk caps and source-age limits remain unchanged. Typed no-HTTP proof never authorizes a retry, terminal release or financial-history rewrite. Profitability remains unestablished.
