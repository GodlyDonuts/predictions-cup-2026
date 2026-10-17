# Throughput and capital release — October17

Source027abb60 (published main source84925305) passed872 actual checks and a
locked optimized ARM64 release at00:29:42UTC. Binary33005b45, unchanged financial
engineb5b28a47. Production rollout is tracked separately; this build proof alone
is not deployment, a fill, or profitability evidence.

The changes address measured execution bottlenecks:

- Keep one bounded immutable historical-order snapshot/index per runtime thread,
  keyed by the exact Session, manifest and durable head. A changed head/session
  invalidates it; oversized histories use the uncached path. No account receipt,
  contract, authority or clock validity is cached.
- Rotate ordinary nonurgent SELL quotes with BUY quotes. Actual FIFO deadlines,
  reviewed inventory economics and revocations retain strict exit priority.
- After a genuine fresh ordinary book, repeat the existing negative-only screen
  before seven account preflight reads. Unknown/stale/managed/opposite-exposure
  cases retain full preflight. It is a rejection optimization, not execution proof.
- Recover the original active nine-review generation across a binary-only base
  change. The original active overlay/request/source metadata stay immutable.
  Every other parsed Control field must match; twelve financial/source/settings
  drift cases are rejected. Root deployment still separately reviews the actual
  new binary/control and the stopped single-worker boundary.

[Validation](proof/validation.json) records872 passes: native90, SDK233,
runtime230, worker119, snapshot6, priority15, maker33, distribution25,
acquisition10, fundamental111. One old native benchmark is ignored. Native and
SDK/runtime/worker core runs are retained from R9 only after exact unchanged
production-source verification; the new focused groups, release and native
identity probe are actual R10 runs. Fixture failures in earlier builds were
retained privately, corrected in source, and are not counted as passes.

[Raw checksums](proof/SHA256SUMS.json) bind16 original files, including actual
compiler/test logs. The raw build manifest records actual uploaded bytes;
Git normalizes Windows CRLF line endings. Root separately verified all258
main source files against the actual build:60 exact raw blobs,198 differing
only by CRLF-to-LF normalization, zero other source differences. Actual binary
and raw-source hashes remain the build provenance, not normalized substitutes.

The original financial bank/nativeb5/all13 bridges, losses, fills and daily
windows are unchanged. The current active generation includes NV872/KS993/TX885
plus the original six. Their prospective principal/quantity scenarios do not
promise fills or calibrated returns. Routine local reservations include drafts.
Official account100-read/30-write per-minute limits are shared by all keys;
this worker retains its24-write headroom and one serialized account queue.

The first R10 deployment draft could not stage because the running Transport
holds the SDK lock for its lifetime. R11 removes the impossible pre-stop quiet
check only. It still requires genuine quiet SDK inspection after graceful
old-worker exit under the capture lock and immediately before promotion.
No second account client or unlocked-success surrogate is introduced.
