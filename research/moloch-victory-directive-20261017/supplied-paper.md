# Moloch Victory Directive: From a Binary Trading Bot to a Tournament-Optimized Prediction-Market Engine

## Executive mandate

Moloch has reached the point where **more engineering around ordinary buy/sell execution is no longer the highest-value work**. The latest `main` I audited is `698969d5d963d34b732726026a9ec8d4eaa3c559`, committed October 17 at 04:03 UTC. The deployed trading source remains `a557c779` with binary `7d44e839` and native engine `7a74f8c3`; the repository is ahead primarily because it contains subsequent economic audits and documentation. fileciteturn31file0L1-L2

The deployed system is no longer capital-starved. The newest source-bound observation reports **9,638.400 SUSQies cash and 85,801.675 of owned cost**, with about **89.9% of cash-plus-cost already deployed**. Two post-upgrade fundamental orders alone added 7,438 shares and 6,453.985 SUSQies of principal. fileciteturn2file0L1-L2 Your supplied holdings CSV tells the same story from an earlier point: approximately 79,102 SUSQies of cost proxy across 24 holdings, with about **60.2% concentrated in the five largest positions and 92.0% in the ten largest**. The old problem of “70k sitting idle” has therefore been replaced by a more dangerous one:

> **Moloch is deploying most of its capital, but it has not yet proved that the deployed capital is attached to calibrated alpha.**

That distinction is now existential. The independent FIFO audit shows that the only completed body of new ordinary trading is **−184.97 SUSQies realized**: 41 ordinary BUY groups, 45,511 acquired shares, 22 losing groups, four winners and 15 flat. Forced `maximumAge` closes lost 216.415 SUSQies while opposite-side BUY netting recovered 31.445. Reviewed fundamental positions had **zero realized P&L** at the audit boundary because they remained open. fileciteturn22file0L1-L2

Meanwhile, the tournament does not reward Sharpe ratio, trade count, elegant software, or even expected SUSQies in isolation. Every participant started with 100,000 SUSQies; trading closes at **noon ET on November 4**, and final ranking is determined by verified SUSQie balance after all markets settle. First, second and third pay $30,000, $5,000 and $2,500 respectively. citeturn27search5turn27search7

That changes the optimization problem.

**The directive for Codex should therefore be:**

> **Stop optimizing Moloch as a generic market maker. Build Moloch into a multi-strategy election probability engine whose objective is maximizing the probability of finishing first, subject to explicit survival and model-risk constraints. Every new component must either create independently measurable alpha, unlock a SIG-native economic mechanism, improve the portfolio’s joint terminal payoff distribution, or be rejected.**

No research plan can guarantee a win. What can be forced is a development process that stops confusing activity with advantage and explicitly optimizes for the competition that is actually being played.

## Where Moloch really stands

The repository is considerably more sophisticated than the currently active strategy set.

The current handoff already contains a source-bound external-innovation model, correlated probabilistic portfolio optimizer, tournament-prize utility model, value surfaces, election modeling components, package infrastructure, execution-distribution machinery and strategy-aware routing. But substantial portions remain research-only, uncalibrated, or disconnected from physical execution. The deployed feature state explicitly has `collateralAccounting=false`, `executionDistribution=null`, `executionModel=null`, no reviewed package allocations, no trusted package certificates and no claimed package fill. fileciteturn21file0L1-L2

This is the crucial architecture gap:

| Capability | Exists conceptually/in source | Economically active now? | Directive |
|---|---:|---:|---|
| Ordinary paired-spread maker | Yes | **Yes, and historically losing** | Quarantine |
| Reviewed terminal fundamentals | Yes | **Yes** | Recalibrate and optimize jointly |
| Kalshi data collector | Yes | Data only | Promote to causal alpha research |
| External catch-up model | Yes | No | **P0** |
| SIG-local dislocation model | Yes | No | **P0** |
| Correlated election portfolio | Yes | No | **P1** |
| Semantic/combinatorial packages | Large infrastructure | No qualified certificates | **P0** |
| Atomic multi-leg execution | Supported | No profitable live strategy | Use for structural opportunities |
| Collateral accounting | Implemented defensively | Off | Opportunistic only |
| Tournament utility | Offline | No execution authority | **Make central allocator** |
| Election nowcast | Research/synthetic validation | No | **P0 before election night** |
| Empirical execution model | Infrastructure exists | Off/unqualified | Build from owned observations |

The current `source_innovation.rs` is already architecturally close to what is needed. It explicitly separates `ExternalCatchUp` from `SigLocalDislocation`, uses causal source ordering, log-odds innovations, common-factor adjustments, decay, uncertainty and conservative valuation. What it does **not** possess is a validated live model or order authority. fileciteturn16file0L1-L2

Likewise, `probabilistic_portfolio.rs` already evaluates scenarios, expected increment, capital opportunity cost, CVaR and factor constraints instead of pretending correlated election positions are independent. But it explicitly does not manufacture borrowing or guaranteed-arbitrage authority. fileciteturn17file0L1-L2

And `prize_utility.rs` goes further still: it already represents opponent terminal wealth, the tournament prize ladder, tie rules and whole-account outcomes. Yet it remains an offline conditional scorer. fileciteturn18file0L1-L2

**That is the leap forward.** The repository has pieces of an institutional-quality decision stack, but production is still behaving mostly like a collection of individual trades.

The goal should now be:

\[
\text{Data}
\rightarrow
\text{Joint posterior}
\rightarrow
\text{Alpha candidates}
\rightarrow
\text{Tournament utility}
\rightarrow
\text{Portfolio allocation}
\rightarrow
\text{Execution-mode selection}
\rightarrow
\text{Outcome learning}
\]

rather than:

\[
\text{Book looks attractive}
\rightarrow
\text{BUY}
\rightarrow
\text{eventually SELL}
\]

SIG itself makes conventional microsecond HFT the wrong mental model. Tournament market realtime updates arrive in **one-second batches**, and normal accounts share a 100-read/30-write-per-minute API budget across their keys. citeturn27search2turn27search8 The important low-latency advantage here is therefore **being the first system to turn new public information into a correct probability and executable portfolio**, not shaving another 100 microseconds from Rust.

## Change the objective from trading profit to probability of winning

The most important change is not a strategy. It is the allocator.

Moloch currently has an offline tournament-prize scorer, but trade generation is still largely driven by local expected economics. That is wrong when the prize function is discontinuous.

The terminal objective should become approximately:

\[
U(a)=
30{,}000P(R_a=1)
+
5{,}000P(R_a=2)
+
2{,}500P(R_a=3)
-\lambda_1P(W_a<W_{\text{floor}})
-\lambda_2\operatorname{CVaR}_\alpha(L_a)
\]

where \(a\) is an entire proposed portfolio adjustment, \(R_a\) is terminal rank and \(W_a\) terminal SUSQie wealth.

This does **not** mean gambling recklessly.

It means recognizing that a portfolio which moves expected wealth from 95k to 105k with tiny variance may be objectively inferior, for this contest, to a portfolio that has lower certainty but a materially larger probability of reaching a prize-winning terminal wealth level. The official prize ladder is extremely top-heavy: first pays six times second and twelve times third. citeturn27search6turn27search7

Codex should promote `prize_utility` from an offline diagnostic into the **top-level candidate-ranking layer**.

Every proposed trade should eventually expose at least:

```text
expected_terminal_wealth_delta
p_terminal_gain
p_terminal_loss
incremental_cvar
factor_exposure_delta
capital_time
p_top_1_delta
p_top_3_delta
model_uncertainty
execution_uncertainty
strategy_id
```

Then Moloch should compare **doing nothing** against every candidate and combination.

This is also where intentionally higher-risk plays belong.

A high-variance position is acceptable only when the model demonstrates something stronger than “we are behind”:

\[
\Delta P(\text{win}) > 0
\]

under a robust range of probability, execution and opponent-terminal-wealth assumptions.

A negative-mean lottery ticket should **not** become acceptable merely because it can produce a huge payout. But late in a winner-take-most tournament, a somewhat lower expected-wealth portfolio can rationally beat a conservative portfolio if its probability mass is moved into the region that actually wins a prize.

That distinction should become an explicit policy state:

| Mode | Objective | When permitted |
|---|---|---|
| **Survival** | Preserve terminal wealth | Model uncertainty high |
| **Alpha** | Maximize expected terminal wealth | Proven/qualified edge |
| **Catch-up** | Maximize top-three probability | Prize gap makes ordinary EV growth insufficient |
| **Attack** | Maximize first-place probability | Late tournament, top-three alone has low utility |
| **Lock** | Protect achieved ranking | If terminal scenarios show sufficient lead |

The optimizer—not the operator—should determine transitions from posterior tournament state.

Final standings remain provisional until all contracts settle, so Moloch must never regard displayed leaderboard wealth as guaranteed opponent terminal wealth. citeturn27search4turn27search5

## Build an alpha stack that competitors cannot reduce to “buy or sell”

The next generation should consist of several independent economic engines competing for the same capital.

**The highest-priority engine is semantic/combinatorial arbitrage.**

SIG gives each binary YES/NO pair complementary economics: buying the opposite side of a held position cancels one pair and immediately pays one SUSQie; selling unheld YES is economically represented as buying NO at the complement. citeturn27search0 Moloch should generalize this across logically related contracts.

For an exactly-one-of-\(k\) family with executable YES bids \(b_i\), consider atomically buying each corresponding NO. The cost is:

\[
C=\sum_i(1-b_i)=k-\sum_i b_i
\]

If exactly one YES can resolve true, the portfolio pays \(k-1\). Its ordinary-state surplus is therefore:

\[
\Pi=\sum_i b_i-1
\]

Conversely, if exactly one YES must resolve true and executable YES asks are \(a_i\), buying the whole YES basket has:

\[
\Pi=1-\sum_i a_i
\]

This is only a starting theorem. Fusion candidates, independent candidates, refunds, cancellations, runoffs and overlapping predicates can destroy the “exactly one” assumption. Moloch must therefore compile **actual resolution rules into payoff states** before declaring anything arbitrage.

The existing relationship parser is already appropriately skeptical: repository code explicitly refuses to interpret a relationship bound as a guaranteed payout floor or an available collateral advance as ordinary cash. fileciteturn12file0L1-L2

The required new component is a `SemanticArbCompiler`:

```text
official contract rules
        ↓
canonical predicates
        ↓
admissible joint states
        ↓
per-contract payoff vectors
        ↓
full-depth executable prices
        ↓
integer / finite-state search
        ↓
worst-state net payoff
        ↓
atomic execution certificate
```

The scanner should search not only party pairs, but race outcomes versus chamber control, state/race implications, mutually exclusive candidates, party winner families, boolean implications, cross-office consistency and any venue-provided relationships.

The venue capability is unusually useful here. Moloch's audited API work found participant-accessible multi-leg execution with up to ten legs, including an explicit `allOrNone` mode in which every leg must execute completely immediately or nothing executes/rests. Default multi-leg execution is less safe because partial fills/resting legs remain possible. fileciteturn37file0L1-L24

That should be treated as a **financial primitive**, not merely an API convenience.

**The second engine is Kalshi-led price discovery.**

Your repository now has 234 direct party references and three separate candidate analogs covering all 237 SIG identities with some form of external information. The three analogs remain intentionally non-equivalent, and none of the mappings itself constitutes pricing authority. fileciteturn33file0L1-L2

The current collector already uses the Basic-tier budget at approximately 20 authenticated GETs per second and receives WebSocket data. A complete 237-book REST rotation is about 11.85 seconds before overhead, so REST polling should be the bootstrap/resynchronization layer—not the fast signal. fileciteturn36file0L1-L2

The production alpha model should estimate:

\[
\Delta K_t
=
\Delta\operatorname{logit}(p^\text{Kalshi}_t)
-
\beta_K\Delta F_t
\]

\[
\Delta S_t
=
\Delta\operatorname{logit}(p^\text{SIG}_t)
-
\beta_S\Delta F_t
\]

and learn:

\[
E[\Delta S_{t:t+h}\mid \Delta K_t,\Delta S_t,X_t]
\]

where \(F_t\) is the common national/state election factor and \(X_t\) includes depth, spread, update origin, recency, market type and semantic mapping class.

This creates two genuinely different strategies:

| Signal | Interpretation | Trade hypothesis |
|---|---|---|
| Kalshi/public source moves first, SIG does not | **External catch-up** | SIG should move toward new information |
| SIG moves while external value/common factors do not | **Local dislocation** | SIG may mean-revert |
| Both move coherently | Information already priced | Do not chase |
| Venues disagree persistently | Structural basis or semantic mismatch | Slow model / abstain |

Moloch already has this conceptual distinction in `source_innovation.rs`; the task is to **fit it on genuine timestamped data, qualify it prospectively, and give successful signals an execution path**. fileciteturn16file0L1-L2

There is already tantalizing but insufficient evidence. An October 16 historical audit found three direct-reference SIG/Kalshi gaps that survived an arbitrary 0.05/share model-risk haircut: NC Republican Senate NO retained roughly 0.022/share, Maine Democratic Governor YES about 0.008, and Oregon Republican Senate NO about 0.006. Across the small fresh subset, 19/14/3/0 apparent opportunities survived assumed haircuts of 0.005/0.02/0.05/0.10 respectively. The report correctly labels these historical leads rather than tradeable or calibrated alpha. fileciteturn35file0L1-L2

That finding should not be ignored. It says there is enough disagreement to justify serious causal modeling.

**The third engine is hierarchical election relative value.**

Individual races should cease being modeled independently.

Use a joint latent model such as:

\[
M_i=
\alpha_i+
\beta_iN+
\gamma_iS_{s(i)}+
\eta_iO_{o(i)}
+\epsilon_i
\]

with national \(N\), state \(S\), office/type factor \(O\), and idiosyncratic race uncertainty.

Then Moloch can ask a much more powerful question than “Is Republican YES cheap?”:

> Is this contract cheap relative to every other observation of the same election factors?

The resulting portfolio can buy a mispriced residual while neutralizing national/state factor risk elsewhere.

The same joint posterior should price chamber control. Senate or House control is not another independent market—it is a nonlinear function of the joint distribution of constituent races. A Monte Carlo posterior over seats can therefore create a **race-to-chamber relative-value engine**.

Moloch's existing probabilistic portfolio optimizer already contains factor limits, expected gain and tail-loss calculations; it needs a live, calibrated scenario generator rather than more optimization code. fileciteturn17file0L1-L2

**The fourth engine is election-night nowcasting.**

This could become Moloch's largest information advantage.

Trading remains open until noon ET on November 4—well after election-night results begin arriving. citeturn27search1turn27search5 That creates an extended interval in which public partial returns can be converted into estimates of outstanding vote better than a trader looking only at reported percentages.

The model should estimate remaining votes conditional on:

- geography,
- reporting mode,
- turnout,
- partisan baseline,
- already-reported precinct/county composition,
- mail/early/election-day sequencing,
- correlated statewide and national surprises,
- uncertainty in outstanding vote.

Every new public result becomes a Bayesian update to **all related races and chamber-control probabilities**, not just the race whose vote total changed.

The repository admits that the present nowcast implementation has relied on synthetic correctness fixtures rather than genuine historical as-of partial-return vintages. That makes nowcast validation one of the highest-priority unfinished tasks. fileciteturn20file0L1-L2

There must be no temporal leakage: a historical replay receives only information that was genuinely available at that historical timestamp.

And all information must be public. The competition permits API automation, but contest rules and fair-play requirements still apply; Moloch should never ingest material nonpublic information or attempt to bypass private participant data. citeturn27search0turn27search5

## Exploit SIG-native mechanics, but stop hunting for imaginary leverage

The answer to your earlier collateral/loan question is now clearer.

**There is no ordinary loan or margin strategy.** SIG explicitly says competition accounts are cash accounts, participants cannot trade on margin or use leverage, cannot trade beyond available SUSQies and cannot allow their balance to become negative. citeturn27search5

The repository's discovered collateral machinery is subtler. The participant API exposes account `availableAdvance`, buying power and `/portfolio/collateral`; the engine can recognize components with a guaranteed payout floor and report advances against them. But the project's own schema audit correctly distinguishes this from ordinary borrowed money: `availableAdvance` is platform-computed capacity associated with eligible components, while outstanding advance is a liability, and a nullable `guaranteedPayoutFloor` is not automatically a new-trade guarantee. fileciteturn14file0L1-L2

So the correct policy is:

> **Do not build a loan strategy. Build portfolios whose guaranteed floors may make them eligible for collateral recycling, then consume additional buying power only when SIG itself reports it.**

Current production should not assume this benefit. The latest feature state has collateral accounting off, and an earlier retained census reported no usable relationships/collateral components. fileciteturn21file0L1-L2

Codex should keep a very cheap relationship/collateral census alive. If a positive official component appears, escalate. If it remains empty, no more engineering time goes into speculative leverage.

The other UI transaction categories also need to be demystified. The audited participant specification does **not** expose participant Signal Purchase or Signal Lease endpoints, and participant API keys cannot create markets or invoke administrative functions. fileciteturn20file0L1-L2 citeturn27search0 Those ledger categories are therefore not secret strategies waiting to be called.

The SIG mechanisms that **are** strategically useful are:

| Mechanism | Strategic use |
|---|---|
| Buy YES / Buy NO | Directional exposure |
| Covered SELL | Liquidation/queue choice |
| Opposite-side BUY | Position closure with immediate pair redemption |
| Rest both sides | Passive liquidity without self-matching |
| Atomic multi-leg AON | Eliminate package legging risk |
| Relationship graph | Discover venue-recognized logical structure |
| Constraint feed | Nominate possible structural inconsistencies |
| Collateral floor/advance | Recycle capital **only when officially recognized** |
| N/A/cancellation refund | Model as a distinct terminal state |
| Realtime market channels | Event-driven rather than polling-only decisions |

Refunds deserve special attention. A cancelled or N/A contract does **not** pay binary 0/1; SIG refunds the refundable cost of the remaining held shares. citeturn27search4 Consequently, Moloch should no longer compile contracts as merely:

\[
\{0,1\}
\]

but as:

\[
\{0,\;1,\;\text{refund(cost basis)},\;\text{other rule-specific states}\}
\]

That creates the possibility of **refund optionality**: a seemingly risky contract can have an asymmetric downside when one exceptional state refunds basis, while a cross-venue reference may resolve differently. This is not free profit; it is a richer state space that most simplistic prediction-market bots are likely to ignore.

Another immediate improvement is the exit router. The platform explicitly supports opposite-side purchases that cancel held shares and immediately return one SUSQie per pair. citeturn27search0 Moloch already contains opposite-BUY logic, and its historical ordinary accounting shows that opposite-BUY netting was profitable overall while maximum-age covered sales were materially negative. That is not proof that netting is inherently better, but it is enough to require an explicit comparison on every exit. fileciteturn22file0L1-L2

For each holding, compute:

\[
V_{\text{exit}}=
\max
\begin{cases}
\text{direct covered SELL value}\\
\text{opposite BUY + pair redemption value}\\
\text{posterior value of continued hold}\\
\text{strategy-specific delayed liquidation value}
\end{cases}
\]

subject to depth, fill probability, signal decay, capital cost and terminal risk.

An arbitrary timer must never make that decision by itself.

## Codex execution directive

The next Codex run should receive a mandate resembling the following. This is intentionally stricter than an ordinary feature backlog.

> **MISSION**
>
> Turn Moloch from a collection of trading policies into a tournament-optimized multi-strategy probability engine. Preserve the sole financial authority, exact journals, reconciliation and existing safety invariants. Do not rewrite the backend or pursue generic infrastructure optimization unless required for a strategy acceptance criterion.
>
> **NON-NEGOTIABLE ECONOMIC RULE**
>
> No feature is “done” because it compiles, passes tests, produces orders, deploys capital, or generates unrealized account value. A strategy is promoted only after it produces a reproducible opportunity funnel:
>
> `source event → model prediction → executable opportunity → admission → order → fill/non-fill → exit/settlement → attributed economic result`.
>
> **ORDINARY MAKER POLICY**
>
> Stop meaningful new capital allocation to the existing paired-spread strategy. Preserve reconciliation and existing inventory exits. Keep a very small experimental/shadow lane only for execution-model data until adverse-selection and complete lifecycle economics become positive prospectively. Do not infer that removing the 60-second timer alone solves the observed losses.
>
> **BUILD ORDER**
>
> First, build the semantic-arbitrage scanner and exact joint payoff certificates over current SIG contracts.
>
> In parallel, convert Kalshi WebSocket innovations into a timestamped prospective alpha dataset and fit `ExternalCatchUp` and `SigLocalDislocation`.
>
> Then bind the live correlated election posterior to `probabilistic_portfolio`.
>
> Promote `prize_utility` into the final capital allocator.
>
> Build and historically validate the public election-night nowcaster before November 3.
>
> **NEVER** call an operator probability a calibrated bound.
>
> **NEVER** treat a Kalshi price as settlement-equivalent without a rule mapping.
>
> **NEVER** call an ACK a fill.
>
> **NEVER** call a collateral advance profit.
>
> **NEVER** infer an atomic package from independent ordinary fills.
>
> **NEVER** increase risk merely because the account is behind.
>
> **NEVER** optimize a strategy on the same events used to evaluate it.
>
> **NEVER** silently extend source/review validity to keep a trade eligible.
>
> **THE ONLY REASON TO TAKE MORE RISK IS THAT A ROBUST TOURNAMENT MODEL SHOWS THAT THE RISK INCREASES PRIZE-WINNING PROBABILITY.**

I would split execution into the following campaign.

| Window | Mandatory deliverable | Promotion criterion |
|---|---|---|
| **Immediate** | Quarantine losing ordinary entry allocation; preserve exits/evidence | No new meaningful principal enters unqualified maker lane |
| **Next 24h** | `SemanticArbCompiler` + full-depth scanner + AON package simulator | Can enumerate worst-state payoff including refund/exception states |
| **Next 24–48h** | Kalshi/SIG prospective innovation recorder and causal replay | Every prediction has immutable first-available timestamps |
| **Next 48–72h** | Fit external-catch-up/local-dislocation models | Positive prospective net edge after depth, uncertainty and basis reserve |
| **Next 3–5d** | Live joint election posterior + factor-neutral relative-value scanner | Calibration and held-out likelihood/Brier tests beat documented baseline |
| **Next 5–10d** | Tournament-state allocator | Every candidate reports Δ expected wealth, CVaR, ΔP(top3), ΔP(first) |
| **Before election week** | Genuine historical election-night replay | Zero future leakage; calibrated residuals on held-out elections |
| **Election night** | Public-results nowcast + event-driven portfolio router | Posterior updates and executable decisions remain inside a bounded latency budget |
| **Final hours** | Terminal wealth/prize optimizer | Allocate exclusively by terminal states, executable depth and final-rank utility |

The workstreams should be isolated enough that failure in one does not stall the others.

The **semantic scanner** should write opportunities even when it finds zero trades. A zero-result scan is evidence.

The **Kalshi model** should score every innovation, including ones on which Moloch abstains. Otherwise only successful-looking episodes will enter the dataset.

The **portfolio model** should maintain a no-trade counterfactual.

The **tournament allocator** must preserve a do-nothing alternative.

The **nowcaster** must preserve the exact information timestamp seen by the historical simulation.

And every live strategy needs its own immutable `strategy_id`, so realized terminal economics can never again be confused with inherited positions, ordinary maker activity, or dashboard marks.

## Capital and risk policy for the remainder of the Cup

The latest repository state invalidates the premise that cash utilization itself is holding Moloch back. Approximately 89.9% of cash-plus-owned-cost was already allocated at the latest source-bound observation. fileciteturn2file0L1-L2

The attached holdings CSV also shows considerable concentration. By my calculation from the file you supplied, the five largest positions contain about **60%** of cost proxy and the ten largest roughly **92%**. Several are economically related Republican/Democratic election outcomes, meaning nominally different markets may still fail together under a common national or state surprise.

So the instruction should **not** be “use all remaining cash.”

It should be:

> **Make every SUSQie compete for capital. Existing holdings receive no grandfathered privilege.**

For position \(j\), Moloch should periodically calculate:

\[
\text{HoldScore}_j=
\frac{
E[\Delta W_j]
+
\kappa_1\Delta P(\text{top 3})
+
\kappa_2\Delta P(\text{first})
}{
\text{capital-time}
}
-\text{tail contribution}
-\text{model uncertainty}
\]

A new opportunity can then replace a weak existing holding rather than requiring free cash.

I would use four capital classes.

**Structural capital** gets highest priority. A genuinely certified positive worst-state package should be allowed to consume substantial available capital up to depth because its economics are fundamentally different from a subjective forecast. Near the end of the competition, the opportunity cost of locking capital until settlement also falls sharply.

**Calibrated alpha capital** goes to cross-venue innovations, factor residuals and well-calibrated election forecasts. Scale it gradually based on prospective results, not one large successful trade.

**Research capital** remains deliberately small. New models earn additional allocation through actual outcomes.

**Tournament-convex capital** is the explicitly aggressive sleeve. It exists to change rank probabilities, not to masquerade as ordinary positive-EV trading.

A reasonable initial control policy—not a mathematically proven optimum—is to prevent unvalidated tournament-convex positions from exceeding roughly **10–20% of current modeled terminal wealth**. The allocator can raise that ceiling later, potentially substantially, only when three things are simultaneously true:

1. the baseline probability of achieving a prize with the conservative portfolio has become very low;
2. a calibrated candidate materially raises \(P(\text{top 3})\) or \(P(\text{first})\);
3. robust perturbations to its probabilities still justify the allocation.

That is the form of “riskier play” I recommend.

Not larger random directional bets.

Not 100% capital utilization.

Not leverage that SIG does not allow.

**Controlled convexity.**

The best convex opportunities will often be low-priced states that Moloch believes are substantially more probable than the market does, or portfolios whose payoff is concentrated in a coherent election scenario. But the joint scenario model must prevent ten apparently separate “underdog” positions from secretly being the same national-factor bet.

A useful allocator should therefore calculate not merely position concentration but **scenario concentration**:

\[
\text{Loss}(s)
=
W_0-W(s)
\]

for every modeled election state \(s\).

The question is not “How many markets do we own?”

It is:

> **Under which few election worlds do we lose everything simultaneously?**

That is especially important now because current capital is heavily deployed.

## Definition of victory for the remaining engineering campaign

Moloch should no longer be judged by the number of features implemented.

By October 17 it already has enough components to create the illusion of sophistication: Rust execution, exact accounting, Kalshi collection, source-bound reviews, value surfaces, probabilistic portfolios, tournament utility, package infrastructure, collateral parsing and nowcast code. The repository itself repeatedly and correctly warns that code presence is not activation and activation is not alpha. fileciteturn2file0L1-L2

The next milestone should therefore be economic:

**Moloch must discover, rank and execute multiple fundamentally different classes of opportunity without an operator preselecting the trade.**

A successful next-stage report should look like this:

| Metric | Required meaning |
|---|---|
| Semantic opportunities scanned | Complete state-certified search, including zero results |
| Guaranteed packages found | Positive minimum payoff after executable depth/cost |
| External innovations observed | Timestamp-causal Kalshi/public events |
| Catch-up predictions | Prospective, not retrospectively selected |
| Local-dislocation predictions | Separately attributed |
| Signal calibration | Predicted move versus actual subsequent executable move |
| Execution quality | Fill quantity, latency, markout, adverse selection |
| Capital-time return | Net terminal/executable gain divided by genuinely occupied capital-time |
| Relative-value portfolios | Jointly evaluated, not independent legs |
| Model calibration | Held-out Brier/log loss/calibration curve |
| Tournament impact | ΔP(top3) and ΔP(first) versus no trade |
| Realized strategy P&L | Exact FIFO/settlement attribution |
| Open terminal value | Separately reported, never called realized profit |

The ordinary maker strategy has already failed its first economic test and should lose its privileged status. fileciteturn22file0L1-L2

The fundamental strategy has deployed meaningful capital but has not yet earned the right to call its operator probabilities calibrated.

The Kalshi layer has enough market coverage to become a serious information system, but has not yet proved lead-lag alpha. fileciteturn33file0L1-L2

The semantic/package system has unusually powerful venue primitives but no qualified profitable certificates yet. fileciteturn21file0L1-L2

The tournament optimizer exists, but it is not yet deciding what Moloch trades. fileciteturn18file0L1-L2

The election-night system could become the strongest late-game information engine, but its genuine historical as-of validation is unfinished. fileciteturn20file0L1-L2

**Those are not five unrelated shortcomings. They point to one architectural conclusion:**

> **Moloch's next leap is to become a portfolio intelligence system rather than a trading loop.**

The execution backend should increasingly become boring. It should receive candidates from competing alpha engines, apply exact authority/risk checks, choose the best execution method, and journal the result.

The intelligence layer should become aggressive:

\[
\boxed{
\text{Public information}
+
\text{cross-venue inference}
+
\text{semantic logic}
+
\text{joint election posterior}
+
\text{nowcasting}
+
\text{tournament game theory}
}
\]

And the allocator should be ruthless:

\[
\boxed{
\text{No demonstrated edge}
\Rightarrow
\text{no meaningful capital}
}
\]

\[
\boxed{
\text{Certified structural gain}
\Rightarrow
\text{attack available depth}
}
\]

\[
\boxed{
\text{Calibrated information advantage}
\Rightarrow
\text{scale with evidence}
}
\]

\[
\boxed{
\text{Large tournament deficit}
\Rightarrow
\text{seek controlled convexity, not random risk}
}
\]

That is the route by which the sophistication already present in the repository can finally become a competitive advantage rather than merely sophisticated infrastructure.