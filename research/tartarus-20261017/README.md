# Tartarus: whole-account winning-capacity work

The October17 executive plan and implementation mandate propose a new decision objective: search for feasible whole-account campaigns that can beat an uncertain opposing terminal wealth. The implementation starts at the existing system's boundaries. It retains Moloch's bank, original lots, sole account worker, account limiter and physical execution controls.

## Findings reproduced on refreshed main

- `semantic_states.rs:105` accepts at most six contracts; the existing prize evaluator requires every holding. A separate sampled full-account matrix is necessary for23 holdings. The local exact semantic compiler remains useful for small contractual proofs.
- `prize_utility.rs:52` requires a complete field and caps opponents at128. A partial public ranking cannot truthfully meet that requirement for a much larger contest. The new field path models declared opposing maximums and an explicit unknown-field envelope.
- `probabilistic_portfolio.rs:150` filters BUYs against expected payout; lines170–171 apply a nonnegative minimum-mean constraint and retain one maximum-mean plan. The new rank-risk factory preserves alternatives before applying monetary-expectation classification; the existing positive-alpha production lane remains intact.

The shared [research interface](CONTRACTS.md) was frozen atbf604d17 before parallel implementations. Checked new modules include [whole-account scenarios](../../moloch/tartarus/account_scenarios.py), [rival-field models](../../moloch/tartarus/rival_field.py) and the [mixed-integer target frontier](../../moloch/tartarus/target_frontier.py). All44 combined tests pass with the pinned private solver, including a cross-module23-holding/incomplete-field/negative-mean campaign. The source still needs actual-evidence adapters and a qualified candidate-to-runtime path. No Tartarus candidate is activated by these modules.

## Independent dated native holdings audit

Root verified the original sealed native report at **523602 / October17 18:13:47.933UTC**, raw SHA256 `3f95693ad82bba9e03863e12bffa2402d4c0b68a6e7afecb0388a76aa505f178`. It contains23 nonempty held sides and149,477 absolute shares. Exact remaining FIFO principal sums to91,851.730, matching the native owned-cost field. Cash is3,613.785; planning reservations2,098.445 leave1,515.340 available before subsequent events. This independently reproduces the paper's holding count/share total from native evidence, without claiming to have inspected its unavailable CSV.

For those fixed holdings, cash plus every absolute share paying one is153,090.785. This is a loose static bound excluding future execution, debt and fees; the modeled outcomes may not be jointly attainable. It is not expected wealth, liquid cash, current account value or a bound on future reallocation. Native active-order count6 does not establish six current physical orders or zero obligations. Complete current official liabilities, actual open-order branches and resolver bindings still require the existing worker's evidence. No bank is rewritten to match an export.

The report's quoted CSV cost-proxy discrepancy244.42397592 remains unresolved here because the CSV/transaction export and reference optimizer ZIP were not attached. The supplied PDF/Markdown formulas are available; those missing files are not claimed as reviewed or executed. Engineering proceeds with explicit synthetic fixtures and original native evidence.

## Current scope

Account scenarios retain every held contract, remaining FIFO refund principal, cash and explicit debt; unmodeled obligations reject. Rival models support an incomplete1,676-participant example with128 observed opponents, joint scenario-dependent thresholds and unknown overtakers. The target solver is a separate rank-risk research path: negative monetary expectation stays visible, unexecuted sale proceeds never fund purchases, numerical solutions receive an exact integer funding/depth/wealth check, and no-action remains available.

Required next outputs are an actual-evidence scenario adapter/capacity census, nondominated target alternatives and explicit rejection reasons, followed by continuation-aware rotation and the plan's regime/source/event-time challengers. Declared scenario probabilities are not calibrated winning chances. Research flags and source hashes do not mint execution approval. [Task173](https://github.com/GodlyDonuts/predictions-cup-2026/issues/173).

## Reproduced synthetic frontier, not trading returns

The paper's all-cash example is reproduced exactly: buying983,982 shares at.08 costs78,718.56, leaving16,018.44 from hypothetical94,737cash. A winning payoff is1,000,000.44. With declared6% probability, incremental monetary expectation is−19,679.64; with12% it is+39,359.28. The rank-risk path preserves the negative-mean alternative while labeling it, and neither case is qualified positive alpha or an executable instruction. The real dated cash is3,613.785 and its available acquisition cash is lower after reservations. Insufficient funding/depth produces an analytic capacity rejection or explicit inconclusive result.

The numerical solver's candidate quantities, depth prefixes, gross funding and target hits are independently rechecked with exact integers. A1ppm positive-target constraint is expressed in integer ppm rather than a floating tolerance that could accept a zero-hit incumbent. Timed-out/nonoptimal searches never establish exhaustive optimality. The returned frontier is a bounded set of nondominated candidates, not a universal optimum or a complete execution model.

Private research dependencies are NumPy2.3.5 and SciPy1.17.0, installed outside the repository and primary runtime. With those on a task-localPYTHONPATH, run `python -m unittest discover -s moloch/tartarus -p 'test_*.py' -q`. Accounting/source validation remains usable without SciPy; a missing solver produces an explicit unavailable outcome. No production service uses this research dependency environment.
