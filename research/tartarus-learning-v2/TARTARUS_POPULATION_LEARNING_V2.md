# Tartarus: Wealth-Relative Population Learning

## A hybrid reinforcement-learning and evolutionary-search specification

**Prepared:** 18 October 2026  
**Repository reference inspected:** `9bcb9873a7ea5b7e423f4e23d37ee2a564a8cced`  
**Status:** Proposed experiment and implementation contract. No trading model was trained, no market-simulator throughput was measured, and no repository, account, service or live mandate was changed in preparing this document.

This supersedes the learning/search design in the earlier `CODEX_LEARNED_POLICY_EXPERIMENT.md`. Preserve that document's source-integrity, execution-accounting, data-split and production-boundary requirements. The new contribution is a wealth-relative, liquidity-aware action interface and a multiscale population search that combines gradient learning, evolutionary exploration and preservation of distinct behaviors.

## Executive decision

Build a system that learns trading behavior rather than merely tuning a menu of existing human strategies. Treat the three venue histories as synchronized observations of related markets; SIG remains the execution venue in this experiment. Kalshi and Polymarket provide reference information, not assumed funding or hedge accounts.

The proposed training design is:

**Causal multi-venue replay + exact account mechanics + uncertain execution models → recurrent policy learning → multiscale evolutionary selection → protected behavioral archive → untouched evaluation → existing Rust execution.**

The research question is whether this combination improves out-of-sample portfolio outcomes and conditional tournament utility at actual usable size. Neither a high simulated score nor a sophisticated architecture establishes a real-market optimum or a future win.

There are three important interpretations of the user's proposal. Normalizing wealth is useful, but liquidity and fixed caps prevent perfect scale invariance. Fast simulated experience is useful, but a completed episode is not a fully trained model. Small, medium and rare large mutations are useful, but they must be combined with independent lineages and behavior-based diversity rather than copying one lucky winner indefinitely.

All numerical model, population, mutation and training settings below are initial experimental defaults. They are not measured optimal settings for SIG, promises about compute time or live trading authorizations.

## 1. Translate the proposal without losing its intent

| User's idea | Proposed implementation | Important distinction |
|---|---|---|
| Treat total money as one whole | Normalize observations and decisions by a documented current wealth reference | Never round away the original balance or replace integer accounting |
| Spend a proportion | Choose target portfolio exposures and wealth-relative child-order budgets | Whole wealth differs from spendable cash; repeated target decisions must not repeatedly spend the same fraction |
| Replay history as fast live data | An event-driven simulator with independent virtual and wall clocks | Simulated time, latency, liquidity and rate budgets remain realistic even when wall-clock execution is fast |
| Let it change many parameters | Learn economic actions; evolve training hyperparameters and selected policy modules | The trader cannot change fills, fees, causal timestamps, test data or the external evaluation objective |
| Most children mutate a little; fewer mutate a lot | Multiscale offspring operators inside a population-based outer loop | Mutation magnitude is measured by changes in behavior, not just raw weight noise |
| Avoid getting stuck on the first success | Independent islands, protected behavior niches, repeated evaluation and fresh lineages | Many nearly identical clones are not meaningful exploration |

Population-Based Training supplies the pattern of training several agents while periodically transferring useful checkpoints and varying hyperparameters. Quality-diversity methods preserve multiple high-performing behaviors instead of a single winner. These are methodological foundations, not SIG profitability evidence. [1, 2, 3]

## 2. Wealth-relative decisions with exact accounting

### 2.1 Keep two representations

The financial engine retains cash, shares, original lots, fees, reservations and liabilities in its existing exact units. The network receives normalized features. The inspected execution-distribution implementation already distinguishes integer monetary accounting from floating-point descriptive statistics; reuse that separation. [R1]

Let `C_t` be actual cash, `R_t` actual cash reservations, `D_t` modeled liabilities and `V_ref,t` a documented, externally priced value of held assets. Define:

`W_t = C_t + V_ref,t - D_t`.

`W_t` is an observation/sizing reference, not a promise that every asset can be liquidated for that amount. The mark construction excludes the policy's own resting quotes; stale or incomplete marks carry explicit availability indicators. Do not silently change from bid valuation to midpoint or acquisition cost because it improves a result.

For a cash-only borrowing-disabled configuration, available cash begins with `C_t - R_t`; other encumbrances are applied according to the actual account rules. Reservations are not also subtracted from total wealth: they are restricted cash, not an additional realized loss. Do not count pending sales as cash.

Normalize such features as:

- Cash fraction, reservation fraction and liability fraction.
- Position value fraction and outcome payout exposure divided by wealth.
- Entry basis divided by wealth, where needed for refunds and accounting.
- Executable book notional divided by wealth.
- Order-size and per-market-cap capacity divided by wealth.
- Log wealth relative to starting wealth, target wealth divided by current wealth, and time remaining.

Retain prices in natural probability units, tick size, integer lot size, source ages and rate-budget state. A zero, negative or unavailable denominator produces an explicit state handled by the validated account process; do not create a giant allocation by dividing through a tiny arbitrary epsilon.

### 2.2 Prefer targets over repeated fixed-fraction purchases

The portfolio head outputs a desired exposure, for example `w_i*=0.04` for a particular held-side asset. This is not an instruction to spend 4% again on every tick. The order head sees current positions and outstanding orders and proposes only the change toward the target.

A child order can use a budget `f_order * W_t`. Its actual integer quantity is determined from the selected limit, fees, cash, existing commitments, source-valid liquidity assumptions and the real per-order cap. It must never be computed at midpoint and funded at a more expensive limit without rechecking.

For an ordinary fixed-price BUY, a simplified bound is:

`q <= floor(min(f_order * W_t, available cash, remaining commitment, remaining market allowance) / worst_case_per_share_cost)`.

Then enforce the actual share limit. Immediate execution additionally requires depth at modeled arrival; a resting limit may fill partially or not at all. For SELLs, the order must be covered by unencumbered holdings. Full target weights plus cash form a portfolio budget, while orders represent transitions toward that budget. A feasibility projection must expose both the requested and admitted action and the reason for adjustment.

### 2.3 Fractional representation does not make capacity scale

Illustration: 2% of 100,000 at a 0.40 price asks for 5,000 shares. At 1,000,000 it asks for 50,000. A book with 6,000 relevant shares has not become ten times deeper. Fixed order caps and write budgets also do not scale with wealth.

Train and evaluate across starting bankrolls, initially 25,000 through 2,000,000 with a log-uniform curriculum and explicit checkpoints at 25k, 100k, 400k, 1m and 2m. These are proposed stress points, not current available funds. Keep real recorded depth fixed when varying bankroll. Artificially scaling every book alongside the bankroll would hide the principal difficulty.

Include both all-cash and actual-shaped inventory starts. Synthetic initial inventories must be feasible, causally specified and clearly tagged; arbitrary future-informed winning holdings are not legitimate starting states.

Two different tests are necessary. A pure accounting unit-rescaling test should preserve actions after conversion. A bankroll-capacity test should allow the policy to behave differently as its desired orders become large relative to unchanged liquidity.

## 3. Construct one causal multi-venue environment

### 3.1 Use a common calendar, not three independent samples

Compile the retained SIG, Kalshi and Polymarket data into synchronized episode windows. Every observation retains venue, contract identity, original source packet, publisher time when known, receipt time, processing time, sequence information and coverage state.

The policy only receives observations available by its simulated information time. Sorting exclusively by publisher time would leak information the live process did not have. A missing publisher clock stays missing. Later corrections do not rewrite earlier observed states.

Keep exact payout-equivalent, contextually related and uncertain mappings distinct. Preserve unresolved exceptions and resolution differences. Reference-venue probabilities are features or declared forecast inputs, not true labels. External last prices are not executable SIG prices.

Do not independently shuffle races from different dates into one portfolio without labeling the result synthetic. Shared political events are part of the task. Train-set block bootstraps preserve synchronized groups where possible. Any source dropout or time-shift augmentation must remain causal.

### 3.2 Three separate simulator clocks

The exchange/event clock governs matching, source events, expirations and settlement. The agent-information clock governs what the policy has actually observed. The machine wall clock measures training speed.

The simulator advances directly to the next event: a source update, order arrival, fill, acknowledgment, cancellation, expiry, rate-budget replenishment or planned policy wake-up. It never sleeps to reproduce a delay. It nevertheless applies the delay in virtual time.

A thousand virtual decisions during a single exchange minute do not create a thousand permitted writes. Rate-limit state persists across orders and is not reset when a policy changes mode or a training fragment ends. Existing configured account limits are loaded from source-bound configuration; do not replace them with a more favorable approximation. Batch and single-order costs follow verified endpoint semantics.

An event-driven policy can choose when to reevaluate, but zero-duration self-loops are disallowed. For a finite tournament objective, use elapsed time and remaining horizon explicitly. Do not make impatience depend accidentally on the arbitrary number of replay rows. Auxiliary discounted tasks must use a documented clock-aware discount.

### 3.3 Exact mechanics, uncertain responses

Known accounting and supported order mechanics remain deterministic and testable. Queue priority, counterfactual replenishment, indirect impact and opponents' responses remain modeled uncertainties.

For marketable orders, use available liquidity at simulated arrival, not at the earlier decision. Consumed depth cannot reappear because an unchanged snapshot is read again. SIG YES and NO views sharing the same underlying liquidity also share depletion. Cancel requests do not erase fills that occurred before cancellation became effective.

For passive orders, use queue/partial-fill scenarios calibrated from supported own-order evidence where available. Unobserved queue positions remain intervals or explicitly modeled distributions. Fills and subsequent adverse movement are jointly conditioned. Sampling favorable fill outcomes independently of price dynamics can erase adverse selection.

Historical replay does not identify all other traders' responses to the learner's hypothetical orders. That limitation is especially important at large size. Use replay for evidence-supported small participation and separate impact/reactive stress models for larger actions. ABM or learned-dynamics additions are hypotheses, not automatic realism. JAX-LOB documents this replay/reactivity distinction. [6]

### 3.4 Don't train through the production filesystem

Authenticate and decode original files once into indexed arrays or columnar shards. Share read-only market arrays across environments; maintain separate positions, outstanding orders, reservations, synthetic queue state and consumed-liquidity state per environment.

No per-step HTTP, decompression, JSON reparsing, financial-journal fsync or historical startup replay belongs in the training inner loop. Preserve enough compact trajectory evidence to reproduce selected episodes. The fast simulator must pass differential tests against the existing exact Rust accounting boundary; it is not allowed to invent a more favorable ledger for speed.

Keep a CPU/Rust reference implementation and one fast vectorized implementation where measured throughput warrants it. Do not commission simultaneous unrelated full rewrites in several frameworks.

## 4. Define an expressive policy rather than a strategy selector

### 4.1 Observation encoder

Use a shared per-contract encoder for causal sequences of venue-specific prices, depth, recent changes, volatility/activity summaries, source ages and disagreement. A permutation-aware set or attention layer combines contracts with portfolio state and remaining competitive horizon. A compact recurrent state retains path information.

Begin with a compact actor-critic, roughly 0.5–3 million parameters as a candidate range, subject to actual memory/inference measurements. Size is an experimental hyperparameter, not a prestige target. Feed exact full depth to execution; a compact network representation may use top levels plus tail summaries but cannot make omitted depth free liquidity.

Do not expose future packet IDs, hindsight regimes, eventual winners or final revised source values as observations. Legitimate contract metadata can be ablated against opaque-ID shortcuts to detect memorization.

Self-supervised pretraining may forecast later observable price/liquidity features or reconstruct causally masked inputs, using training data only. Keep a from-scratch population branch. If all agents inherit one frozen erroneous representation, varied heads may not provide much real diversity.

### 4.2 Action heads

A proposed autoregressive action consists of a market or verified basket, supported action family, side, target exposure fraction, child quantity fraction, limit-price aggressiveness and resting lifetime/cancellation decision. Include no-action, hold, reduce and cancel. Allow full-range economic choices within the actual approved mandate rather than embedding a fixed preference for safety.

Portfolio and execution heads share information. At first, they are heads of one policy, not separately hardcoded strategy families. A learned patience decision and sparse event-driven wake-up schedule can later supply temporal abstraction if it improves matched-compute results.

Physical masks represent real feasibility: funded cash, available inventory, genuine API order types, open obligations, ownership and source eligibility. Do not mask a trade because the old forecaster dislikes its expected return or the old maker has no matching quote. Otherwise the purported learned system is still trapped in the old candidate generator.

This does not authorize removal of current production controls. A separate tested learned-intent interface is necessary wherever current economic admission rules would otherwise prevent evaluation of a new policy. The inspected rank evaluator mixes real mechanics with explicit campaign assumptions; do not equate calling it with providing a general RL action interface. [R2]

### 4.3 Critic and learning targets

Use an actor-critic baseline with recurrent PPO in the interactive simulator. PPO trajectories must come from the corresponding current policy; frozen historical behavior actions are not an on-policy rollout. [4]

A distributional auxiliary critic can represent normalized monetary return. Add a separate target-conditioned head for `P(W_T > H | information, policy)` where outcomes are observable or the scenario assumptions are explicitly declared. A binary win-return distribution alone contains only the success probability; it does not describe monetary downside. Keep both outputs distinct. [9]

Model uncertainty and policy randomness also differ. An ensemble's disagreement is a diagnostic; it is not automatically a calibrated probability interval.

## 5. Inner learning and outer evolution

### 5.1 Inner loop: gradient learning

Each policy gathers fresh simulator trajectories and updates its actor/critic over training blocks. This learns which sequential actions performed well and uses credit assignment across time. Inner settings include learning rate, exploration/entropy coefficient, advantage-estimation settings, update epochs, recurrent context and auxiliary-loss weights.

Do not mutate a million-weight network after every individual order or reset it after every episode. A training block contains several episodes or truncations, enough outcomes for comparison, and a specified gradient-update budget.

### 5.2 Outer loop: population-based learning

Maintain an initial logical population of 32 policies in four islands of eight. The number concurrently resident on the GPU is a benchmark decision; 32 logical lineages need not mean 32 models and all their optimizer states resident simultaneously.

Periodically compare compatible learners on a common search-development suite. Retain promising checkpoints, replace selected weak slots, and generate children with the mutation mixture below. Do not compare a newly reset child against a fully matured parent after one noisy episode. Give reset and structural-change branches a recorded grace budget.

PBT-style checkpoint transfer changes weights and training schedules during optimization. Evolution is not a replacement for gradient learning here; it allocates search around and away from useful lineages. [1]

### 5.3 Proposed multiscale mutation mixture

The percentages apply to replacement/offspring slots across many selection cycles, not every policy in every episode. Protected elites are stored separately.

| Offspring operator | Initial share | Intended effect |
|---|---:|---|
| Local | 50% | Nearby hyperparameter updates and small output-controlled perturbations |
| Medium | 25% | Larger changes to policy heads, training horizon, exploration schedule or context |
| Large | 15% | Broad behavioral change through head/module perturbations and larger hyperparameter changes |
| Head/partial reset | 5% | Preserve useful representations while starting a substantially new action or value mapping |
| Fresh lineage | 5% | New seed, or separately budgeted alternate compact architecture, independent of the leading family |

These fractions are proposed priors. Compare them against uniform-scale mutations and independent restarts. Later adapt operator allocation using repeated search-development improvements per unit compute, while retaining a nonzero exploration floor. A single lucky high-return child must not transfer the entire search budget to its mutation operator.

### 5.4 Measure mutation power in behavior space

A fixed weight-noise standard deviation does not mean a fixed behavioral change. Networks differ in layer scaling and sensitivity; recurrent components can be particularly brittle. Parameter-space exploration and sensitivity-aware mutation research support measuring the resulting policy change. [5, 7]

On a pinned reference batch of training-only market histories, compare the parent's and child's masked action distributions, normalized quantities, price offsets and patience decisions. Recompute recurrent state through the same burn-in separately for each policy. Do not reuse the parent's hidden state as the child's history.

For categorical action families, an average KL divergence is a useful component. For bounded exposure and quantity outputs, use a normalized action-distance component. Alternatively compute the divergence of the autoregressive joint action distribution where tractable. Report both pre-projection and executed-action changes so a feasibility projection cannot hide enormous raw mutations.

Illustrative starting KL bands for the categorical component are `1e-3`, `1e-2`, and `1e-1` for local, medium and large exploration. They are not financial risk levels or a universal calibration. Rescale a proposed perturbation by line search toward the intended behavioral band. Validate invariants and finite outputs; reject broken policies, not merely aggressive economic behavior.

Prefer head/module perturbations or sensitivity-normalized weights to blindly applying large isotropic noise throughout a pretrained recurrent encoder. Neural-network weight crossover is not enabled by default because independently trained units need not align.

### 5.5 Use successful directions without imprisoning the search

An optional evolution-strategy branch can test mirrored head perturbations `theta + sigma*epsilon` and `theta - sigma*epsilon` on the same exogenous episodes. Differences in fitness estimate which parameter directions were useful. [8]

An illustrative estimator is:

`g_ES = (1 / (2*N*sigma)) * sum_i [(F(theta+sigma*eps_i) - F(theta-sigma*eps_i)) * eps_i]`.

Use this only for a defined search objective and compatible architecture. Larger operators, archive parents and fresh lineages still explore directions not favored by this local estimate. Restricting all mutations to the last good direction would reproduce the premature-convergence problem.

Do not automatically spend equal compute implementing several ES variants. First compare this branch to pure PPO, PPO with PBT and the proposed mixed search under equal measured budgets.

### 5.6 Checkpoint transfers must be algorithmically valid

A checkpoint includes encoder, actor, critic, observation normalization, architecture ID, optimizer state, training objective, action adapter, simulator version and source/split identities. Copy compatible bundles rather than actor weights alone.

After an actor mutation, architecture change or parent replacement, discard old on-policy trajectories. Collect new ones from that exact child and its action distribution. Reinitialize or deliberately transform optimizer moments for changed parameters; never unknowingly preserve moments associated with unrelated weights. A changed architecture starts a fresh compatible optimizer. Value heads may require a burn-in/refit interval.

Population scores and gradient losses are not directly comparable when members use different training auxiliaries. Outer evaluation uses the same fixed economic/target metrics for all candidates.

## 6. Protect behavioral diversity, not just parameter diversity

### 6.1 Why mutation magnitude alone is insufficient

Fifty children of one parent can all discover the same trades even if their weights differ substantially. Distinct seeds can also converge to the same profitable-looking historical episode.

Maintain a quality-diversity archive using behavior measured on fixed search-development histories. MAP-Elites and population-based quality-diversity work supply this idea; the proposed market application still needs validation. [2, 3]

Candidate descriptors include holding-time quantiles, exposure concentration, passive-versus-aggressive participation, turnover/capital-time, sensitivity to external-source removal, cash commitment pattern and action sequence similarity. Descriptors are measurements, not prescribed strategy classes.

A sparse archive with a few continuous descriptors is preferable to an enormous manually designed grid. Cap archive size and periodically rescore members. Stale noisy champions are not permanently correct.

### 6.2 Diversity must pass an economic quality test

Keep separate research niches and a deployment-eligible archive. A behavior can be worth preserving as a stepping stone without being acceptable for live use. Novelty alone cannot qualify it for deployment.

Use multiple independent episodes and queue seeds to reassess promising archive members. Track action-descriptor variability and economic uncertainty. Research on uncertain quality-diversity highlights that lucky evaluations can contaminate an archive; reevaluation is part of search, not an afterthought. [10]

Four islands use different seeds and training-block resampling but the same external evaluation protocol. Allow occasional migrations of useful, different behaviors. Limit reproduction of a single family when lineage diversity collapses. Keep at least some fresh/from-scratch representation branches.

Do not average all archived actions at deployment. Opposing actions can cancel each other into a policy that no member learned. Select a verified checkpoint first; a learned selector or distillation is a separately evaluated later experiment, including switching costs and ownership of open positions.

## 7. Reward design without rewarding self-deception

The mission objective is fixed outside the population. A policy may vary its behavior or learning settings; it may not pick the evaluation objective, scenario probabilities, costs or test set that makes it win.

For the historical capital-building task, one useful score is externally valued net wealth change divided by starting wealth, compared with no-action from the same initial holdings. Report absolute money and liquidity alongside normalized return. Another training branch can use log wealth growth where positive wealth is guaranteed by the task definition; it is not a substitute for the final first-place objective.

For tournament training, terminal utility is the actual stated rank objective, such as a strict first-place indicator with documented ties. Unknown future market outcomes and opponent wealth come from declared scenario models, not observed truth. Keep per-model results separate. Current unresolved elections cannot become new real outcomes simply because thousands of simulated tournaments were run.

A return distribution and target-conditioned critic help expose average outcome versus rare target attainment. They do not make an assumed scenario generator calibrated. Monetary loss, near-ruin probability, attainable size and winning-probability sensitivity remain visible even when the chosen objective tolerates high variance.

Use a curriculum of bankrolls, initial inventories, remaining horizons and target ratios. Short price-replay episodes train capital-building/control skills; longer scenario episodes train terminal ranking. Never report success on easier target ratios as a solved million-point comeback. Final comparison uses actual account state and current benchmark assumptions.

Potential-based shaping is optional, only with matching discount and terminal boundaries so it does not silently change the objective. Training auxiliary losses or exploration bonuses are removed from the external evaluation score. Reward for trade count, cash released, positive FIFO closes alone or own-quote price marks is prohibited because it can improve while whole-account value worsens.

## 8. Full-history training without validation contamination

“All available history” means all eligible development history, not all history including the test. Freeze the chronology before feature preprocessing, dynamics calibration, simulator fitting, mutation scheduling or policy selection.

Use four roles:

1. Fit/training data for representation, simulator calibration and policy gradients.
2. Search-development data for PBT selection, mutation adaptation and archive descriptors. This is part of optimization, not untouched evidence.
3. A locked chronological test for a predeclared small number of frozen finalists.
4. A new live-forward cohort, whose subsequent outcomes are not known during training.

Purging must include label, holding and recurrent burn-in overlap. All three venue clocks and correlated event clusters are considered. Model scalers fit on training only. Reset recurrent memories at true episode boundaries; do not carry future context from one sampled window into another.

Population selection is a form of hyperparameter search. A million trials can overfit a development set even if gradient updates never touch it. Keep every trial/lineage in the experiment register. Do not use the locked test as an evolutionary fitness server.

Compare policies on paired exogenous paths and event-keyed random execution draws where feasible. Random numbers keyed by event and mechanism avoid accidental loss of comparability when different policies take different numbers of actions. Add independent-seed evaluation because one favorable seed family is not enough.

Use event/time-block uncertainty and independent model seeds. Report distributions and confidence intervals, not only one best run. RL evaluation research gives good reasons not to rank methods solely from a small number of noisy point estimates. [11]

## 9. Simulation fidelity under wide exploration

The broader the policy's permitted action space, the more important the following falsification suite becomes:

| Test | Failure it detects |
|---|---|
| Unchanged snapshot after prior purchase | Reusing consumed liquidity |
| Simultaneous YES/NO views | Double-counting the same matching resource |
| Decision-to-arrival price move | Filling at a price no longer available |
| Partial fill then cancellation race | Erasing exposure or crediting nonexistent cash |
| Data gap and later recovery | Treating missing history as known good execution |
| Large bankroll, fixed recorded depth | False scale invariance |
| Tiny order versus economic-size order | Mistaking one-share alpha for usable capacity |
| Stale external feed or source disagreement | Treating reference quotes as truth |
| Own resting quote removed from valuation | Mark-based reward manipulation |
| Nonpredictive price process with costs | Accounting, selection or leakage artifacts |
| Opponent or settlement model changed | Overfitting a favorable scenario generator |
| Protocol version and cloned policy changed | Invalid reuse of on-policy trajectories |

Calibrate exact deterministic mechanisms against native Rust; compare probabilistic fills against observed own-order cohorts with adequate support. Evaluate pessimistic and optimistic execution bounds separately. Large unsupported actions need impact/reactive sensitivity, not an invented precise fill distribution.

A domain-randomization trainer samples models from a frozen, documented range. The policy cannot alter those ranges. An optional later adversarial simulator selects difficult but evidence-compatible conditions; it is not allowed to invent arbitrary impossible prices just to defeat the policy.

Stress-test the trained policy in simulator variants that were not selected by the policy's own search. If returns disappear outside one assumed replenishment model, the result is simulator dependence, not demonstrated alpha.

## 10. Measure speed correctly

Accelerated replay is the right mechanism. A subsecond episode may be feasible for a compact compiled environment, but complete population training is a different quantity. No benchmark for this proposed SIG environment exists in this deliverable. GPU-parallel order-book simulation is supported by prior work, not a promise about this machine or dataset. [6]

A useful wall-time budget is:

`T_total = T_decode + T_compile + T_rollout + T_gradient_updates + T_selection + T_final_evaluation`.

Illustrative arithmetic only: one day at one portfolio decision per second is 86,400 decisions. One pass through 32 policies is 2,764,800 policy-decisions, excluding multiple queue seeds and market-message processing. At a hypothetical effective end-to-end rate of 100,000 policy-decisions/s that is 27.648 seconds before extra training or selection work; at 1,000,000/s it is 2.7648 seconds. These rates are not measured or expected performance claims.

Measure market messages/s, portfolio decisions/s, transitions/s, gradient-update time, episode length, model size, full-depth size, GPU memory and end-to-end wall time. A message-processing rate cannot be reported as a policy-training rate. Include cold decode/compile separately from warmed results.

Independent environments run in parallel, but each policy's causally dependent action sequence still has sequential steps. Keep arrays shared where exogenous history is identical; environment financial and impact state stays independent. The official writer does not run learning jobs.

## 11. Required matched-compute ablations

Run a progressive experiment ladder rather than building every sophisticated component before a checkpoint exists.

| Variant | Mechanism added | Purpose |
|---|---|---|
| A0 | Current strategy and no-action baselines | Reference economics from identical starts |
| A1 | Independently trained recurrent RL seeds | Establish the benefit and variance of policy learning |
| A2 | PBT with one perturbation scale | Test whether population transfer helps |
| A3 | Multiscale mutation mixture | Directly test the user's exploration idea |
| A4 | Protected behavior archive and islands | Test premature-convergence protection |
| A5 | Behavior-controlled perturbation | Test whether raw weight noise was destructive |
| A6 | Bankroll/depth-conditioned training | Test scaling rather than assuming it |

Use equal aggregate environment decisions and report actual compute, including selection reevaluations and reset grace periods. Include a matched-wall-time comparison so an elaborate method cannot hide an enormous computational advantage. Use more than one independent search run where available; report when resources are insufficient for a precise ranking.

Pretraining, larger architectures, evolution-strategy branches and learned expert selection are optional later ablations. They are not prerequisites for the first runnable experiment.

## 12. Acceptance artifacts

A completed engineering phase delivers real files, not an interface waiting for an unspecified fitted model:

- A dataset manifest with original hashes, missingness, source chronology and nonoverlapping split membership.
- An exact-mechanics simulator test report and separately labeled execution calibration/stress report.
- A measured single-policy and batched rollout/training benchmark.
- At least one trained policy checkpoint with its entire reproducible bundle.
- A population genealogy with mutation type, measured behavior distance, parentage, optimization budget and evaluation history.
- A behavior archive whose members have reevaluation and uncertainty records.
- A matched-compute ablation report, including unfavorable and null results.
- A frozen finalist report on locked data and then new forward observations.

Read-only shadow inference validates timing and predictions but cannot certify hypothetical passive fills. Any later physical validation uses the existing actual account process and mandate. No model weights, config file, archive score or this document gives a new order permission.

## 13. Repository integration and ownership

At the pinned repository reference, reuse the following verified boundaries:

- `moloch/live-runtime/src/execution_distribution.rs`: actual order-outcome and monetary-unit structures. [R1]
- `moloch/native/src/reviewed_rank_campaign.rs`: real funding, quantities, state assumptions and runtime boundaries; not a generic unrestricted learner interface. [R2]
- `moloch/tartarus/` and the existing scenario/capacity work: comparison baselines and accounting semantics, not obligatory strategy choices.
- The existing SIG, Kalshi and Polymarket retained histories: source inputs, with source-time and mapping limitations preserved.

Suggested NEW paths, not claims that they currently exist:

```
research/tartarus-learning-v2/
  dataset_manifest.json
  split_manifest.json
  episode_compiler.py
  normalized_state.py
  target_action_adapter.py
  reference_env.py
  vector_env.py
  execution_models.py
  policy.py
  train_inner.py
  population.py
  behavior_mutations.py
  archive.py
  evaluate.py
  benchmark.py
  experiment_registry.jsonl
  tests/
```

Separate agents own dataset/splits, environment/normalization, RL training, population/archive and independent evaluation. Freeze observation/action/result schemas early; one integrator owns shared interfaces. Use the repository's current coordination and integration convention rather than creating conflicting execution owners.

The first work block ends with a causal qualified subset, one working simulator, a PPO checkpoint and a benchmark. The second adds PBT and multiscale replacement. The third adds archive/islands and locked comparison. These are engineering dependencies; agents can build upcoming components against synthetic contracts in parallel. Synthetic tests must not be relabeled trained market success.

## 14. Codex master directive

> Implement Tartarus Population Learning v2 as a research-only challenger. The core experiment is recurrent reinforcement learning plus a multiscale population-based outer search and a protected behavioral archive. Preserve exact financial mechanics and the original production writer. Do not restrict the learned actor to old strategy proposals.
>
> Normalize wealth-relative observations and targets, but retain exact account units, spendable-cash constraints and unchanged recorded liquidity. Train across bankroll/depth ratios; do not assume a fraction scales capacity automatically.
>
> Synchronize SIG, Kalshi and Polymarket histories by information availability. Run a virtual event clock with actual rate limits, execution delays, partial fills, shared complementary liquidity and explicit queue/impact uncertainties. Decode once, remove filesystem work from the inner loop and benchmark before claiming seconds-per-training-run.
>
> Train a compact recurrent PPO baseline. Add PBT-style checkpoint transfer and local/medium/large/reset/fresh operators. Preserve independent islands and an archive based on measured behaviors. Calibrate perturbations through output changes; do not scatter identical raw noise over every weight and call it diverse exploration. Discard incompatible on-policy trajectories after mutation or transfer.
>
> Let training hyperparameters and economic choices vary. Keep source semantics, simulator fidelity, test splits, mission score and actual authorization external to the trader. Model-generated tournament outcomes remain conditional assumptions.
>
> Deliver a real checkpoint, measured benchmark, lineage/archive record and matched-compute comparison. The next success criterion is a reproducible improvement on untouched economic evidence at useful size, not code volume, synthetic wins or a promise of optimality.

## Source register

Methods below motivate the experimental design. None establishes an edge for this competition.

[1] Jaderberg et al., *Population Based Training of Neural Networks* (2017). https://arxiv.org/abs/1711.09846

[2] Mouret and Clune, *Illuminating search spaces by mapping elites* (2015). https://arxiv.org/abs/1504.04909

[3] Pierrot and Flajolet, *Evolving Populations of Diverse RL Agents with MAP-Elites* (2023). https://arxiv.org/abs/2303.12803

[4] Schulman et al., *Proximal Policy Optimization Algorithms* (2017). https://arxiv.org/abs/1707.06347

[5] Lehman et al., *Safe Mutations for Deep and Recurrent Neural Networks through Output Gradients* (2017/2018). https://arxiv.org/abs/1712.06563

[6] Frey et al., *JAX-LOB: A GPU-Accelerated limit order book simulator to unlock large scale reinforcement learning for trading* (2023). https://arxiv.org/html/2308.13289v1

[7] Plappert et al., *Parameter Space Noise for Exploration* (2017/2018). https://arxiv.org/abs/1706.01905

[8] Salimans et al., *Evolution Strategies as a Scalable Alternative to Reinforcement Learning* (2017). https://arxiv.org/abs/1703.03864

[9] Bellemare et al., *A Distributional Perspective on Reinforcement Learning* (2017). https://proceedings.mlr.press/v70/bellemare17a.html

[10] Flageat et al., *Extract-QD Framework: A Generic Approach for Quality-Diversity in Noisy, Stochastic or Uncertain Domains* (2025). https://arxiv.org/abs/2502.06585

[11] Agarwal et al., *Deep Reinforcement Learning at the Edge of the Statistical Precipice* (2021). https://arxiv.org/abs/2108.13264

[R1] Pinned execution outcome/accounting source. https://github.com/GodlyDonuts/predictions-cup-2026/blob/9bcb9873a7ea5b7e423f4e23d37ee2a564a8cced/moloch/live-runtime/src/execution_distribution.rs

[R2] Pinned rank-campaign boundary. https://github.com/GodlyDonuts/predictions-cup-2026/blob/9bcb9873a7ea5b7e423f4e23d37ee2a564a8cced/moloch/native/src/reviewed_rank_campaign.rs
