# Cooperative preflight service: rollout draft review

This is a review-only operator draft. No SSH, rollout mode, account request,
compiler, service action or production mutation was performed for this review.
The execution owner retains publication and deployment. No candidate build or
release result is asserted here.

Private draft: `C:/Users/csram/AppData/Local/Moloch/ops/paper6_preflight_service_rollout.py`.
Its raw SHA256 is
`2674d117cc0c2fc9f2ecf2b8caff27d1812f32d86246e3ba029637980000006f`.
The predecessor `paper6_side_edge_rollout.py` remains byte-identical at
`2c4b30e6c12df270315c1e5c9d5ecedb576e67be2ab2300a409aac5aa8dc7da2`.
Neither private operator nor raw rollout receipts belong in Git.

## Fixed boundaries

- Prior owner is PID516085, source
  `870ce889d51a04c654a93785277c47ea33b52bea`, executable
  `d2ad93b50af64122b626bb197ce79bcfcbdf6f22d8ec0e738b11ebf9b6f6a023`.
  The original prior receipt is
  `/tmp/moloch-paper6-side-edge-production-rollout.json`; its actual raw hash,
  control and unit hashes must be frozen before any mode can run.
- Native identity remains
  `8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754`.
  Base source manifest is
  `/tmp/moloch-side-edge-20261016-r1/source-manifest.json`, raw SHA256
  `ed95fa74a9d8a7c798b8e66d1f06d87da9cc935dd079e6f75432aa3fbebef0ca`.
- Exactly248 source inputs remain exactly248. Only
  `moloch/live-runtime/src/worker.rs` may change, to its final reviewed raw hash.
  Every native and other source input remains identical. No bridge, bank copy,
  financial replay migration, history reset or lease deletion is available.
- Control changes only `activation.binarySha256`. Existing
  `ordinarySideEdge=enforce`, capital, daily windows, risk limits, authorization
  and rate settings must compare equal. This is no new trading mandate.
- The existing bounded-restart drop-in and its exact named backup are pinned;
  no new service or alternate process owner is introduced.

## Checks retained

The predecessor's pinned helper provides original dispatch hash-chain verification
and a quiet-account-queue check. Acknowledged, accounted open orders may remain;
this is not an invented all-orders-closed requirement. Unknown acceptance,
unresolved attempts, active queue requests or manual review prevent promotion.

Stage requires the approved manifest hash, genuine final source/build/artifact
pins, original test-group log and optimized-release log, actual binary bytes,
prior receipt/process/executable/control/unit binding, and STOP absence. It saves
immutable release/control/unit candidates while the old worker continues.

Stop is cooperative and bounded. Only after the old process is absent and the
capture lock is exclusively held does it freeze the original head/report/manifest,
full event-prefix bytes/hash, ordinary/fundamental/package journals, and all12
bridge entries/raw bytes. Bridge bytes must also match the prior rollout receipt.

Promote requires that entire stopped boundary remain unchanged, queue quietness,
and a separately execution-owner-reviewed STOP witness. The witness binds actual
STOP/status/request hashes: the old worker may rewrite STOP, so the request token
alone never establishes ownership. Only that verified owned STOP is archived;
global or unrelated STOPs are not cleared. There is one start attempt. Failure
retains/creates STOP with no automatic retry. The receipt records
`freshTradingVerified=false`; successful start is not evidence of fresh trading.

## Pending freeze and validation limits

Final source commit, worker hash, build path, exact test groups/subsets, original
test/release-log hashes, binary hash, source-manifest/validation hashes and actual
prior receipt/control/unit pins are deliberately `None`. `freeze_ready()` rejects
this state before every mode. The execution owner must supply genuine checked
evidence and review the resulting new operator hash; no synthetic value below is
stored as an approved pin or test success.

The draft compiles as Python. **46 synthetic pure guard checks passed** for missing
freeze fields, binary-only controls, enforcement preservation, exact248 source
sets/native immutability, prior binding, test-count/log/release guards and independent
STOP witness. These checks import the draft without invoking its CLI or deployment
functions. They do not test the actual candidate, account state, service or rollout.
