# Tartarus research boundary v1

Frozen initial interface, October17. Task [173](https://github.com/GodlyDonuts/predictions-cup-2026/issues/173). This is an offline research path beside the existing Rust execution system. It does not replace native accounting, weaken its six-contract semantic proofs, mint an approval or activate a strategy.

## Units and identity

Python modules use snake_case and JSON-compatible dictionaries. Every monetary value suffixed `_micros` is an exact integer in millionths of a SUSQie; quantities and ppm weights are integers. Reject booleans/floats in integer fields. Preserve stable scenario IDs and exact contract/exchange/side identity. Do not infer rules or probabilities from matching names.

Account and scenario inputs carry `schema_version: 1`, explicit `evidence_class`, `as_of_ms`, `valid_until_ms`, `source_sha256`, and an assumption/unavailable-state statement. Synthetic fixtures are labeled synthetic. Hashes bind bytes, not independent source authentication or consent. Research output always has `execution_authorized: false` and `calibrated_probability_claimed: false`.

## A. Whole-account compiler

`account_scenarios.compile_account(account, scenarios, decision_ms)` returns a complete modeled account or raises a specific unavailable/rejection error. `fifo_sell(holding, quantity)` is a pure research helper returning remaining lots and removed basis; it never changes an actual bank.

Account fields:

```text
schema_version, evidence_class, as_of_ms, valid_until_ms, source_sha256
cash_micros, reserved_cash_micros, terminal_debt_micros
holdings: [{contract_id, exchange_id, side: YES|NO, quantity,
            lots: [{quantity, unit_cost_micros}], source_sha256}]
pending_obligations: [] or explicitly modeled obligations
```

Cash includes reservations; available acquisition cash is cash minus reservations. Reservations are not an additional terminal expense. Terminal debt is deducted once. Pending obligations must not be silently ignored: the first implementation may reject nonempty unmodeled obligations rather than invent zero liabilities. Reject duplicate/conflicting holdings and quantity/basis mismatches. Both YES and NO may exist only if represented explicitly as different side holdings.

Scenario input:

```text
schema_version, evidence_class, as_of_ms, valid_until_ms, source_sha256
contracts: [{contract_id, exchange_id, semantic_source_sha256}]
scenarios: [{id, kind: ordinary|exceptional,
             payouts: {contract_id: resolved_yes|resolved_no|refund_principal},
             assumption_label}]
unmodeled_state_statement
```

Every held contract and every scenario payout must bind to the registry. RefundPrincipal returns actual remaining FIFO principal for each held side, not quantity or original pre-sale basis. The matrix is sampled/declared, never an exhaustive certificate. Bound dimensions and total cells, allowing at least23 contracts.

Canonical compiler output consumed by other modules:

```text
schema_version: 1
scenario_ids: [ordered IDs]
baseline_wealth_micros: [aligned exact integers]
available_cash_micros, cash_micros, reserved_cash_micros, terminal_debt_micros
holdings, contracts, scenario_payouts: [payout dictionaries aligned to IDs]
as_of_ms, valid_until_ms, source_sha256, evidence_class
unmodeled_state_statement, execution_authorized: false
calibrated_probability_claimed: false
```

## B. Candidate and capacity frontier

`target_frontier.solve_frontier(compiled_account, actions, probability_models, rival_models, constraints)` searches a bounded finite integer problem; it returns the no-action baseline, nondominated alternatives and explicit missing-data/unattainable outcomes. No per-leg positive-EV filter precedes rank-risk search. Keep existing positive-alpha production admission untouched.

Probability models are `{id, weights_ppm: [aligned integer weights summing 1_000_000], assumption_label}`. Rival models are `{id, thresholds_micros: [aligned opposing maximums], assumption_label, complete_field: false}`. Pair them explicitly; never independently shuffle a field model across election scenarios. Fixed target sweeps are declared assumptions, not a current leaderboard query.

Buy actions are `{id, contract_id, exchange_id, side, levels: [{quantity, unit_cost_micros}], fee_per_share_micros, source_sha256, available_at_ms, valid_until_ms}`. Price levels must be valid executable ascending depth; integer fills consume prefixes. Respect actual available cash and explicit gross cost/quantity/loss constraints. Hypothetical or acknowledged sales do not fund purchases. Initial buy-only search may return an explicit rotation prerequisite rather than assume an atomic sale-funded campaign.

Constraints include `decision_ms`, `maximum_gross_cost_micros`, `maximum_total_quantity`, optional finite solver bounds/time limits and target models. Recompute monetary outcomes and integer/depth/funding feasibility exactly after a numerical solver. Missing solver/depth/probability evidence is unavailable, not a zero return or an approved order.

Each alternative retains quantities/depth cost, complete before/after wealth vectors, per-model exact monetary expectation and target probability, minimum wealth/downside, funding, assumptions and source/deadline pins. Negative or mixed monetary expectation remains explicit. A positive declared mean alone is not established positive alpha. Return `execution_authorized: false`; do not create native physical capabilities.

## C. Opposing field summaries

`rival_field.build_models(observation, scenario_ids, case_specs)` creates declared flat, growing, joint factor-correlated and adverse-overtaking threshold models, with an explicit unobserved-field envelope. Support a1,676-participant synthetic field without labeling128 public observations complete. `participant_count` includes us; opponents equal participant_count minus1. Do not invent private portfolios.

Model first-place comparisons using the opposing maximum. Retain top-order-statistic and tie information when supplied; absent ties/prize rules remain unavailable or explicit bounds. Return canonical `thresholds_micros` aligned to scenario IDs for the frontier. Joint dependence is supplied by case specifications, never fitted from hypothetical private positions. Publication clocks and projection horizon remain separate. Exact tie/prize arithmetic should reuse the existing rules concept, with conservative explicit assumptions where original evidence is absent.

## First acceptance cases

Whole account23 holdings; omission fails; partial FIFO refund; cash/debt counted once; joint race dependence. Rank-risk negative-mean candidate survives separately from positive-alpha eligibility; more than the maximum-mean candidate remains; insufficient cash/depth makes a target unavailable; marked equity and sale ACKs are not cash; final integer recheck. Field>128 with incomplete observations, unknown-overtaking envelope and state dependence alter target probability. No test or module establishes a live fill or calibrated winning chance.
