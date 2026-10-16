# Paper 7: optional joint ordinary-maker gate

Source candidate only, from `20543435`; no worker, SDK, native engine, account,
activation or production changes. The author has not compiled these changes.
The paper is a research lead, not authority to adopt its risk allocations.

## Minimal integration

`maker_policy::Settings.jointExecution` is optional and omitted when absent.
Old serialized settings/review hashes, default decisions and legacy ranking
economics retain their original path. Existing maker-policy worker calls need
no new dispatch lane. Actual activation still requires a separately reviewed,
immutable Control and both original source observers/configurations.

Example shape (digests must come from actual reviewed sources):

```json
{
  "jointExecution": {
    "version": 1,
    "distributionConfigSha256": "<actual canonical Config SHA256>",
    "minimumCompleted": 100,
    "minimumTimeBlocks": 20,
    "minimumPerClass": 1
  }
}
```

The production profile refuses fewer than 100 completed observations or 20 time
blocks; thresholds may be higher. Every zero/partial/full class must meet the
explicit `minimumPerClass`. Actual observer configuration minima also apply.
Thresholds are reviewed experimental criteria, not proof of calibration,
independent market draws, out-of-sample alpha or confidence guarantees. No
claimed training examples are created from ACKs, open orders or timeouts.

The only addition in `execution_distribution.rs` is a read-only getter of the
actual configured `Config` and its canonical digest, using existing bank/policy
binding. There is no change to collection, pooling, clocks or label math.

## Actual evidence, then economics

The gate derives the existing exact strategy fingerprint from:

* `kind=ordinary_passive`;
* actual observer Config digest;
* this immutable maker review digest;
* actual native Policy digest.

No arbitrary fitted strategy or old review's pooled observations can be
imported. It queries `estimate_runtime_action` with this exact current native
BUY's scope, side, limit, full requested quantity, creation and expiration, and
the reviewed inventory horizon. Current source absence, stale/gapped books,
wrong configuration/horizon or disabled observer defers entry.

For the optional path, legacy `execution_edge` is used only for the existing
opaque current quote/source eligibility and comparative diagnostics. Neither
its positive estimate nor its legacy negative classification qualifies the
joint model. An actually configured but unobserved new strategy can use only
the original explicitly unqualified exploration lane: at most 20 per order,
100 deployed, original quantity cap and additional 20 realized-loss stop.
These are existing ceilings; settings may be stricter. Missing observers or
current features cannot become invented statistics or a fitted positive edge.

Qualified joint evidence additionally requires:

* all declared observation/block/class thresholds and the estimator's own gate;
* every retained matched observation value-covered, no value censoring;
* no unresolved matched trials or incomplete owned-capital histories;
* finite block error and nonnegative finite uncertainty, tail, write and
  reservation-plus-owned-capital-time charges;
* a finite positive joint score after all those charges and the reviewed
  additional portfolio charge.

The joint score is already per **requested** share and includes zero and partial
fills. Confirmed zero execution does not lose principal. The old
`gross * (1-fill_probability)` full-loss charge is deliberately not added again.
Positive score is rounded down per requested share before integer scaling;
the exact additional portfolio charge is subtracted once. Nonpositive covered
joint economics defers; unavailable score stays unavailable, not zero.

Ranking uses the positive whole-request net result, full gross funding and the
same reviewed inventory horizon. Actual historical capital time is charged in
the joint score. The ranking horizon is a reviewed comparator, not a guarantee
of future execution, capital release or settlement. Owned SELLs and genuinely
covered opposite-side hedges keep their original priority and authority.

The private Decision binds the bank/report, quote, dispatch, review, original
source deadline, actual observer source-cursor/label digest and Config. A current
validation re-queries the real observer and rejects changed source/economics.
The token is not deserializable from diagnostics. `candidate()` continues to
mean a legacy candidate and returns none for joint decisions; the existing
`rank()` handles private joint economics directly. Ordinary physical authority
still checks the exact request, fresh seven receipts, ownership, STOP, account
budget and unknown outcomes. No model creates money or bypasses native caps.

## Authored checks and limits

New checks cover byte-compatible omitted settings, production thresholds,
unknown model fields, missing observer, actual cold-start eligibility and caps,
configuration/source/expiry revocation, positive legacy data failing to create
joint qualification, and conservative joint-charge arithmetic. A genuine
Session/DispatchLog fixture sends original-shaped ACK/order/full-fill-tape
receipts through native feedback and both observers for zero, partial and full
trials, then actual covered SELLs release owned capital. Three complete positive
trials still fail the explicit hundred-trial production threshold. No synthetic
fixture is described as qualified live evidence.

Root must compile and reproduce these tests, run original maker/distribution
regressions and the runtime suite, and review exact immutable settings before
any opt-in. A cold restart starts a new model cohort. This patch does not import
historical fits, assert that sufficient current data exists, enable either
observer, authorize larger exploration or claim profitability.
