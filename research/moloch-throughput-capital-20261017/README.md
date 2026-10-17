# Checked capital and execution candidate — October17,01:16UTC

The next native/runtime candidate passed **892 actual checks** and an optimized
ARM64 release. It is **not yet deployed**. Current production remains sole584167,
with265 official fill rows, cash51,175.35, position cost44,268.335 and combined
attributed realized−136.23. Four new BUY orders acquired28,885 shares for19,587.50;
these holdings have not realized a profit. Full original financial-prefix
compatibility is being checked while production continues trading.

[Implementation and limits](../../docs/MOLOCH_CAPITAL_NATIVE_UPGRADE_20261017.md),
[actual build/validation](native-upgrade-proof/validation.json),
[raw checksums](native-upgrade-proof/SHA256SUMS.json),
[complete original build sources](native-upgrade-proof/actual-source.zip), and
[Git normalization verification](native-upgrade-proof/main-source-normalization-verification.json)
separate checked source from the pending native rollout and prospective eleven
review configuration. Source7c11cd1c corresponds to main4b5dcfd6; all260 raw source
files match those main blobs exactly or by CRLF-to-LF normalization only
(62 exact /198 normalized). Prior failed fixture checks remain retained and are
not counted as passing checks. Per-order10k and account API limits remain.

# Actual restored trading and new fills — October17,01:02UTC

[Live proof](live-proof/live-verification.json) binds the actual new worker,
source/binary/native, healthy trading, recovered original generation, committed
source refresh and new official fill groups. Three filled BUY orders total
12,478.46 principal/18,885 shares/10 fill rows. Cash58,284.39, owned cost37,159.295,
combined attributed realized−136.23. These new holdings are not realized profit.
[Raw pins](live-proof/SHA256SUMS.json), [actual hot-reload CLI](live-proof/reload-001.log)
and [observed exit](live-proof/reload-001-result.json) retain original evidence.

[Latest decision trace](live-proof/decision-trace-latest.json) is one observation:
managed q10790 risk/policy493ms, intent98ms, first-preflight-to-applied-ACK8575ms,
book age atACK2159ms. It is not a controlled speed benchmark or maximum rate.
The native bank can advance ahead of the periodically published progress file;
use the timestamped ledger fill groups, not stale progress counts, for this proof.

# Actual R11 rollout — October17,00:35UTC

[Production receipt](deployment/production-rollout.json) verifies sole584167
started the33005b45 binary/source027abb60 with unchanged nativeb5/13bridges.
[Initial health](deployment/initial-restoring-health.json) proves executable and
service, explicitly **not fresh trading or generation recovery**. Old progress
belongs to577770. [Boundary](deployment/boundary.json), [owner approval](deployment/root-approval.json)
and [frozen operator](deployment/operator.py) bind the genuine requested stop,
quiet SDK and original nine-model generation. No financial history was reset.
[Raw pins](deployment/SHA256SUMS.json) and [main/source normalization verification](deployment/main-source-normalization-verification.json)
retain exact provenance. [Prepared source refresh](deployment/source-refresh-prepared.json)
is offline only, not a committed runtime generation.

The pre-stop SDK helper failure was confined to the unused R10 draft. The actual
R11 removes only the pre-stop lock attempt, then verifies SDK quiet under the
capture lock after the old PID exits and again before promotion. The original
active overlay/request/authority/source metadata survive the binary-only rebase.

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
