# Prospective observer deployment; SIG trading continues

Reviewed source2fa9 was published on main before deployment. Linux validation
at17:36:48–17:36:55UTC reproduced the103 observer,24 witness,14 adapter and22
parent-analysis checks. Root reproduced the same four suites locally. Exact
Git blob bytes were packaged explicitly: the first Windows `git archive`
applied CRLF conversion, so its source failed the exact LF capture-producer
check before testing or activation. That original private archive was retained;
the separately named corrected archive and source hashes appear in
[validation.json](validation.json). No source check was weakened.

At17:38:10, finite read-only V5 PID526213 started with producer2fa9 and the
unchanged map3f8d. SIG PID525450 and its process start identity stayed unchanged.
V4 PID525623 was normally sealed first; its originals are preserved. Source,
receipt, first-availability clocks and the sampling age/fee/horizon policies did
not change. [Actual start](start-v5.json), [prior seal](prior-v4-seal.json),
[fresh observer status](fresh-observer-status.json).

The17:39:29 [SIG verification](full-cycle-verification.json) shows healthy
realtime, fresh trading, matching executable, no STOP and a full-cycle status
that now matches525450. There are13 post-launch order acknowledgments and two
reconciled fill rows,200 cumulative rows, combined realized−131.465000.
Native8acc/full capital remain. No additional account client was launched.

The new versioned verifier also replayed the original frozen V2 inputs once at
nice19,17:38:44–17:40:49UTC,124.798seconds, actual exit0. It selected the fixed
historical200ab sampler and reproduced the **identical original result bytes**,
SHA25626015e2cac4c61a6e8118240a08ebf7f9068bf0bcc5cb0dbb8e70cd8fa6eb095.
[Actual parity receipt](historical-v2-parity.json). New source semantics were not
applied to old inputs. Whole-witness, model-eligibility and financial authority
remain false.

The separately audited original V3 emitted ten rows/seven horizon labels;
[input replay](../../moloch-trade-trigger-20261016/witness/actual-v3/README.md)
passed. V2+V3 has at most twelve rows, so the frozen100-OOS/100-selected minima
cannot be met. Combined analysis was **not run**, rather than reporting a fitted
negative model. The active V5 cohort remains prospective; its eventual original
inputs must be pinned and replayed before evaluation.

The [raw artifact pins](raw-artifact-pins.json) bind the nine original Linux
check/deployment/freshness artifacts. Historical V2 parity is a separately
retained original receipt. No recovered sample rate, model gain, profitable new
entry, avoided toxic fill or general strategy profitability is established by
this deployment.
