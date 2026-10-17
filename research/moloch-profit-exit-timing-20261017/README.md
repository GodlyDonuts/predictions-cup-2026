# Profit exit timing

Task [#164](https://github.com/GodlyDonuts/predictions-cup-2026/issues/164) improves completion of profitable inherited-inventory exits. Production remains R22 while the candidate is qualified.

The existing production worker completed another profitable exit at 08:22:16.186 UTC on October 17: **1,382 NO shares on exchange 948, 1,244.425 SUSQies proceeds, 1,236.890 recorded basis released, and +7.535 realized gain**. All five official fill rows, the original acknowledgement and complete terminal record agree. This gain belongs to R22, before this candidate was deployed. See [the source-bound observation](actual-exit/source-bound-observation.json).

At the frozen 380-fill boundary, combined attributed realized P&L was **−114.490**, comprising −184.970 from new trading and +70.480 inherited gross realized. Cash was 3,597.585 and owned principal 91,867.840. Two purchases totaling 548.600 principal occurred after the earlier 373-fill baseline and are accounted for separately. Open principal is not profit, and these figures do not establish daily dashboard P&L, executable liquidation value or leaderboard rank.

## The timing defect

An original unused 948 NO profit draft, q14579, was created at 08:14:10.963 UTC and expired at 08:14:23.963. An earlier root CLI observation reported an owned-source refresh completing at 08:14:21.052, implying only 2.911 seconds remained. The original status file was overwritten before the finite capture began, so this archive cannot independently authenticate that completion clock. Frozen dispatch requires more than five seconds before beginning its final account packet. The expired-read lane also rejected a draft while it was still live, and its presence suppressed older same-side hints.

The [timing evidence](actual-exit/timing-diagnosis.json) preserves the original quote and coincident native admissions. Multiple admissions share its creation millisecond; the evidence does not identify one particular admission as the cause of creation. There was no intent, attempt or acknowledgement for this draft. Repeated drafts against the same holdings are not additive missed fills or profit. The later successful q14627 demonstrates that the old path could sometimes complete.

## The change

- A current unused inherited Profit draft can nominate reads before activation or near expiry. It supplies no execution permission.
- An already valid genuine successor remains the first choice after all seven original account, market and book facts.
- If no usable successor exists, the worker may retire at most one current, proved-unused inherited draft through the existing durable `not_sent` transition. It rejects any intent, attempt, native binding, fill, managed holding, protected package or conflicting physical liability.
- Normal native reconciliation decides whether to create a successor. A separate post-book binding preserves the original book receipt and admission clocks while recording the actual later planning head. Only a genuinely new qid with current positive worst-lot economics can proceed through its original activation and physical authorization.

No native engine, SDK, source clock, quote lifetime, price, quantity, account rate, model, risk limit or activation is changed. If the planner creates no successor, the worker defers. This change does not force trades or promise an increase in profit.

## Evidence and validation

The original private evidence archive is SHA-256 `11167216c88850ff5414f16415f57e00e4924e255dce2174518c1c9f2b40432f`. Root checked all 57 manifest entries. An independent [verifier](verify-original-evidence.py) and [result](actual-exit/independent-verification.json) authenticated the complete 20,883-row dispatch prefix and a 12,809-row native suffix through both original financial observations, matched all prior 373 fill raw tokens unchanged, and reconciled exact cash, principal and realized deltas. Reproduction uses the original private archive beside that verifier; the raw account records are not published.

Earlier audit failures are preserved privately: an incorrect dispatch genesis assumption, reserialization of arbitrary-precision numbers, and an unsigned assumption about the official SELL tape total. The final audit hashes original bytes and explicitly requires the signed negative SELL quantities. Raw account records remain private.

Candidate source is `43b0341a`, including the delegated planning module at `660143c5`. Actual ARM64 checks passed 134 worker cases and the unchanged 21 focused library cases. Final lifecycle and optimized release qualification continue. Earlier failures are preserved: the initial stage omitted an existing Rust research test fixture, a managed-mode test omitted its actual bank prefix, and a worker test referenced private helpers in a sibling module. These fixture issues were corrected; no production process was stopped for them. Installation and measured production benefit require separate observations.

The [frozen depth census](FROZEN_DEPTH_CENSUS.md) found no second positive inherited nomination at the observed cutoff: only 948 passed; all thirteen other unmanaged inherited sides failed the gain floor. Its 55-share slice is a non-additive hypothetical alternative to an existing unused draft, not another fill or profit.
