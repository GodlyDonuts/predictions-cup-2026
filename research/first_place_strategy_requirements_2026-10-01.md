# Predictions Cup: first-place strategy and bot requirements

Research date: 2026-10-01, approximately 19:14–19:26 UTC. Scope: public sources and existing read-only research only. No account API requests, credentials, orders, cancellations, or external messages were used. This is a development/decision brief, not an instruction to trade now.

**Forecast update:** subsequently verified public model values and source-specific limitations are in `holdings_forecast_evidence_2026-10-01.md` and `holdings_forecast_evidence_2026-10-01.json`. The values are published-model outputs, not a calibrated house probability. Quantitative implementation/sizing belongs to Drogon; this brief specifies evidence and safety requirements.

## Decision

Build three engines in this order: **settlement-logic/structural scan → evidence-calibrated pricing and bounded execution → rank-aware portfolio optimization**. Optimize the probability of finishing first after settlement, with expected prize as a secondary diagnostic. Do not optimize interim marks, raw expected return alone, or a fixed “always diversify”/“always concentrate” rule.

There is **no newly verified executable edge or current fair probability in this report**. The earlier structural scan found no positive executable structural edge in its checked books. Existing card prices, portfolio marks and old book snapshots must not activate trading.

The current portfolio has 94,401.38 SUSQies cash and only 5,598.62 of entry cost exposed. Its no-more-trading terminal range is 94,401.38–112,572.38 under ordinary binary outcomes. A first-place plan needs a defensible estimate of the terminal competition hurdle; neither this range nor the current leaderboard by itself tells us whether it is competitive.

## 1. Verified constraints and source hierarchy

### Official competition rules

The [September 30 rules](https://predictionscup.com/rules/) establish:

- Final ranking uses verified SUSQies after every market resolves; interim standings are provisional
- Trading ends November 4, 2026 at noon EST (17:00 UTC), then positions lock
- One individual account; unlimited bots are permitted subject to API/technical controls
- Participation is free and uses nonredeemable simulated currency
- Sponsor liquidity can disappear, and order mistakes need not be corrected
- Insider/influence-related market restrictions apply; participation is individual
- Sponsor may alter market rules/outcomes; eligibility and prize verification still matter
- Ties share rank and pool the corresponding prizes; no tiebreaker

The [Terms, updated September 22](https://sig.thesuper.market/terms), prohibit collusion, self-trading, circumvention and unfair manipulation. The bot must not chase artificial marks, coordinate balances across accounts, or probe unsupported internals. Ordinary competitive quoting is distinct from manufacturing score or volume.

### Settlement and mechanics

The [settlement guide](https://sig.thesuper.market/docs/settlement-and-payouts) says winning shares pay 1 and losing shares 0. Cancellations/N/A refund the refundable cost of remaining shares. Automatic checks run every four hours; official results can be delayed. Chamber control and runoffs require administrative settlement. Final standings wait for all markets. Consequently, a displayed settlement date is not a promise of tradable cash at that instant.

The [trading guide](https://sig.thesuper.market/docs/markets-and-trading) explains that selling an unheld side creates exposure to the opposite side at the complement price. Buying the opposite side cancels matching held shares and releases 1 per pair immediately. Own orders do not match each other. Therefore the YES and NO views of one order book are not two independent liquidity pools; count each underlying resting order only once.

### Fees and access

Entry/participation costs are zero. A separate simulated transaction-fee schedule was not located; existing fill/cash reconciliation shows no fee in the observed fills, not proof that every possible operation is fee-free. Execution code should have explicit configurable fees and assert ledger reconciliation rather than silently assume the absence of all charges.

The [October 1 changelog](https://sig.thesuper.market/docs/changelog) directs automation to the supported v1 API and states a shared per-account budget of 100 reads and 30 writes per minute; bulk price requests support 100 exchanges. Coordinate all bots through one limiter. Do not multiply keys to circumvent limits. Public API docs were reachable as a documentation shell in this research; detailed execution semantics in the earlier review remain subject to implementation verification.

## 2. Structural package engine

### Build a settlement graph, not a title-matching arbitrage bot

Use the existing inventory as discovery only: 237 contracts grouped into 117 labels, comprising 34 Senate races, 35 governors, 46 House races, and two chamber-control pairs. This is not the full underlying election/seat universe. D/R/Independent labels exist for Montana Senate, Nebraska Senate and Rhode Island Governor. A listed set of parties is not necessarily an exhaustive ballot.

Every contract needs: tournament and exchange IDs; jurisdiction, office, seat and regular/special election; candidate-affiliation predicate; resolution source; finality standard; cancellation rule; rule version/hash; close time; expected settlement window. Keep certified candidate lists and subsequent public corrections with source timestamps.

The prior scan verified an important Info-tab provision in markets 274–276 and 279–281: **a fusion-ticket winner counts for every party on that ticket**. Thus a candidate-level outcome is one winner, but the party-market payout vector need not be one-hot. Never universally enforce “sum of party YES payouts = 1.” Do not assume a party’s Senate victory equals caucus control.

### Candidate packages, conditional on verified predicates

All following costs are volume-weighted executable buy costs including fee/slippage allowance; these are mathematical screening rules, not findings of available trades.

1. **Mutually exclusive outcomes A and B:** NO(A)+NO(B) pays at least 1. Consider only when combined cost is below 1 and both cannot resolve YES, including fusion/affiliation cases. Third-party winners do not break this lower bound; overlapping party predicates do.
2. **Exhaustive alternatives A₁…Aₖ:** their YES basket pays at least 1 if at least one is necessarily true. Incomplete D/R baskets fail this test. A basket can be exhaustive without being exclusive, but overlap must be represented accurately.
3. **Implication A ⇒ B:** NO(A)+YES(B) pays at least 1. A cheaper exposure can dominate a more expensive equivalent one only after settlement equivalence is proved, not because party labels look similar.
4. **Identical predicates across separate exchanges:** compare buying the cheaper exposure with closing/hedging the dearer equivalent, accounting for independent cancellation and rule-change risks.
5. **Seat/chamber constraints:** enumerate the complete seat map, held-over seats and the exact contract definition of control, including independents/caucuses, ties and vacancies. Only then derive joint payoff bounds. An individual competitive seat does not ordinarily imply chamber control.

For each package, enumerate admissible outcomes s and payoff vector a(s). Evaluate min_s[a(s)·q − execution_cost(q)]. Include partial fills and asymmetric cancellation/refund cases separately. A package profitable only when all legs receive ordinary election settlement should be labelled “conditional structural edge,” not risk-free arbitrage. Sponsor override risk is not eliminated by algebra.

The prior public scan is instructive: Massachusetts D/R YES could be bought for 0.995 for a small observed quantity, but the official ballot had other candidates, so its guaranteed payout floor was zero. Nebraska’s independent Dan Osborn must not be treated as a Democratic nominee. Montana’s independent and Democratic candidates are different people. Reuse these as regression tests.

### Execution and capacity

- Walk book depth per underlying order; require synchronized freshness/version checks and reject empty, locked or stale books
- Size by the least available leg, not the largest displayed depth; evaluate every additional tick separately
- Reserve enough cash for the worst permitted partial-fill state and recovery; maintain leg-specific loss limits
- “Atomic request” does not establish simultaneous full execution. Verify order semantics before using this as an arbitrage guarantee
- Predetermine whether an orphan leg is retained as a valid directional position, hedged within a bound, or immediately cancelled/exited. Never chase a missing hedge without a cap
- Rank opportunities by absolute realizable surplus, robustness, capital consumed and cash-release time. A tiny high-percentage opportunity can contribute almost nothing to first place

## 3. Election evidence: usable sources and contamination controls

### Preferred source families

- **VoteHub Legacy:** polling and fundamentals. Its [2026 methodology](https://votehub.com/wp-content/uploads/2026/05/2026_votehub_midterm_methodology.pdf) distinguishes Legacy from Complete; Complete additionally incorporates expert ratings and prediction-market information. It uses election-year holdouts and correlated simulation. Prefer Legacy for the independent forecast baseline; treat Complete-minus-Legacy as a diagnostic. Its [forecast](https://votehub.com/2026-forecast/) and [poll directory](https://votehub.com/polls/all-polls/) are dynamic. Cloudflare verification blocked the cloud browser after one reload; this pass did not retrieve a current probability table and stopped that site.
- **Silver Bulletin FLIPR:** the [public methodology](https://www.natesilver.net/p/flipr-midterms-model-methodology) distinguishes Lite, Classic and Deluxe. Classic combines polls/fundamentals; Deluxe adds expert ratings. It models shared national, demographic, state and race uncertainty and special voting systems. The [current landing page](https://www.natesilver.net/p/nate-silver-2026-midterm-election-polls-model) is dated October 1, but its full forecast is subscriber content. Do not bypass the paywall or assume unrestricted automated export. Public methodology remains useful for model design.
- **Race to the WH:** its [Senate forecast](https://www.racetothewh.com/senate/26) supplies a second model family using polls, historic trends, candidate quality and fundraising, with daily simulations. Current probabilities were subsequently verified visually in its rendered public chart; see the companion evidence dossier. A supported machine-readable export was not verified. Treat methodological transparency/backtest access as a requirement before granting substantial ensemble weight.
- **Cook/Inside Elections/Sabato:** useful for ballot context and race-specific changes, not numerical win probabilities by fiat. [Cook’s Senate index](https://www.cookpolitical.com/analysis/senate) lists September 23 updates; much detailed material is subscriber-only. Expert categories cannot be converted to 60/75/95% without out-of-sample calibration.
- **Primary poll releases:** [Marist](https://maristpoll.marist.edu/latest-polls/), [Emerson](https://emersoncollegepolling.com/), [Quinnipiac](https://poll.qu.edu/search), and state university pollsters. Retrieve methodology/toplines from the original publisher. Aggregators help discover polls, but duplicate releases are one observation.

### Two concrete checks relevant to current holdings

**North Carolina:** [Catawba/YouGov’s September 30 release](https://www.catawba.edu/news/all-news/2026/yougov-39/) reports Cooper 47–35 among 885 likely voters, or 48–37 with leaners. Fieldwork was September 4–15. This is evidence of a substantial polling lead, not a 48% winning probability or fresh September 30 fieldwork. Do not automatically call the existing cheap NO position positive-EV because it has a large payoff multiple.

**Florida:** [Change Research’s September 28 release](https://changeresearch.com/research/2026/2026-09-28-florida-aapor-methodology-statement) reports Nixon 47–48 versus Moody, fielded September 25–27. It names Freedom Project USA as sponsor, uses English-only online recruitment, and inconsistently describes likely-voter counts as 1,063 and 1,107. Flag the inconsistent metadata and downweight/quarantine until reconciled. The [self-funded UNF July release](https://static1.squarespace.com/static/6994d571bce3436c60d86870/t/6a637f8ec8249053ae278f02/1784905614403/UNF%2BPORL%2BSummer%2BStatewide%2B2026%2BFLLV%2B-%2BPress%2BRelease.pdf) had Moody 50–42 over Nixon using 848 likely midterm voters and mixed phone/web methods. It is much older. These contrasting signals justify careful re-estimation, not an invented current fair price.

### Forecast ingestion and calibration requirements (proposed design)

1. Store publication time, fieldwork dates, collection time, forecast as-of time, office/candidate matchup, sample population, n/effective n, mode, sponsor, pollster, revision and URL. Reject future timestamps, wrong cycles, pre-primary matchups and inconsistent identifiers.
2. Maintain a provenance graph: poll → aggregator → model; ratings → Deluxe/Complete; Kalshi → Complete. Combining these descendants as independent observations produces false confidence. Do not multiply their odds.
3. Start with a simple mixture of distinct model families; estimate weights on whole-election-year holdouts. Do not train/test by randomly splitting neighboring races from the same election. If comparable historical forecasts are missing, admit that weights are heuristic and widen uncertainty.
4. Evaluate Brier score, log loss and reliability by probability bin, office and days-to-election. “Percent of winners called correctly” mostly rewards safe-seat coverage. Track the tails where cheap NO/YES positions generate apparent large edges.
5. Calibrate model probabilities jointly with margins and shared errors. Do not use a single poll’s sampling margin of error as the complete election-margin distribution; nonresponse, turnout, common polling error, temporal movement and undecideds remain.
6. Deduplicate repeated releases and overlapping samples; cap a pollster/sponsor family’s contribution. New releases of the same fieldwork are not new independent polls. Downweight age by fieldwork, not headline date.
7. Reject a newly stale source for new orders. Distinguish stale ingestion from naturally sparse polling; sparse races can use fundamentals with more uncertainty. Invalidate on candidate/rule changes and material public news even if a nominal cache TTL has not elapsed.
8. Produce point estimates plus uncertainty ranges and sensitivity runs: each source family removed; sponsored polls excluded; wider shared-error tails; alternative turnout/caucus/cancellation assumptions. No new directional position should depend entirely on one fragile input.

## 4. Joint portfolio and tournament objective

### Scenario engine

Represent each simulated election as a coherent national outcome. A proposed margin model uses a common national shock, correlated regional/demographic components, a state shock shared across offices, and race-specific noise. Fit their magnitudes/covariance from historical residuals; do not invent universal correlation coefficients. Use heavy-tail sensitivity cases and preserve common election-day error as election approaches. Time-dependent campaign drift can shrink; poll bias does not vanish simply because voting starts.

Map candidate outcomes into contract payouts using the verified predicates. Simulate runoffs, ranked-choice and independents where relevant. Generate chamber outcomes from the entire real seat universe and appropriate control definition. A party label on a contest contract is not automatically a caucus assignment.

Given current cash C, positions q and candidate trade costs c, terminal wealth is C − c + sum of position payouts/refunds. Calculate expected terminal wealth, loss quantiles, worst relevant scenario, probability of clearing several terminal score hurdles, and probability of first place only when an opponent model is defensible.

### First place versus expected return

Primary target: maximize P(final rank = 1). Show P(unique first), tie frequency and expected actual prize separately. The simulated currency has no redemption value outside the contest; a strategy with the highest expected account balance need not have the highest chance of beating everyone.

If public competitor holdings are available through authorized supported surfaces, model their terminal payoffs in the **same** election scenarios. If only current marks are visible, use a range of plausible terminal winning hurdles and opponent exposures. Do not present a precisely calibrated first-place probability from current leaderboard scores alone. Report a hurdle-success curve and sensitivity to the rival model.

When ahead in credible terminal scenarios, common exposures or lower variance may preserve the lead. When behind, a concentrated, positively priced scenario can improve winning chances; concentration must be purposeful, not a reflexive late longshot. Portfolio diversification counts independent outcome risks, not number of contract IDs. Democratic Senate control plus several Democratic Senate longshots can be one national-factor bet.

Require concentration reports by candidate/race, state, chamber, national direction and shared model source. Sum worst-case losses on open orders with held inventory. Cost exposure alone misses large terminal-payoff swings from cheap contracts. Avoid cancelling genuine positive edge solely to cosmetically smooth interim equity.

### Quoting engine

For robustly valued liquid markets, compare bounded passive quotes with crossing costs. Quote around the evidence-based fair range, tilted for inventory and tournament objectives. Backtest adverse selection, partial fills and opportunity cost; a visible spread is not collectible free profit. Any unheld sell adds opposite exposure, so enforce the same limits as a buy. Prefer occasional high-quality updates over indiscriminate cancel/repost loops that exhaust writes.

## 5. What the present holdings imply

Use the reconciled holdings in the earlier review, not web-card values:

| Holding | Entry cost | Maximum ordinary payout | Interpretation to test |
|---|---:|---:|---|
| RI Democratic Senate YES 2,072 | 1,999.48 | 2,072 | Small carry upside, potential exit-depth penalty |
| MA Democratic Senate YES 2,061 | 1,999.17 | 2,061 | Small carry upside; assess opportunity cost |
| Florida Democratic Senate YES 12,500 | 1,500.00 | 12,500 | Main existing terminal-upside position |
| NC Democratic Senate NO 1,538 | 99.97 | 1,538 | Cheap contrarian/tail exposure, not proven edge |

RI+MA’s combined maximum gain from entry is only 134.35. Do not add safe-state carry merely because it probably wins. Conversely, do not dump holdings at poor executable bids because the portfolio display marks them down: current proceeds, revised probabilities and alternative uses of cash matter; historical cost is sunk.

Florida’s existing payoff dominates the portfolio’s upside and participates in the national Democratic-success factor. NC NO can pay in opposite national conditions, but cannot be assumed to hedge Florida efficiently or independently. Compare the joint portfolio with and without each position under the same scenario set.

With all other current positions winning but Florida losing, terminal wealth is 100,072.38. With RI/MA winning and Florida/NC losing, it is 98,534.38. These are payoff arithmetic, not probability forecasts. Nothing here justifies increasing any position before fresh book and model checks.

## 6. Time, testing and launch gates

**Capital clock:** distinguish trade cutoff, predicted cash availability and ultimate settlement. Favor faster realization only if the capital can actually be reused before cutoff. Annualizing a small settlement gain is irrelevant to a locked final-rank contest. Election-night information can matter while trading remains open; unofficial calls must not be silently treated as certified settlement.

**Engineering build order:**

1. Canonical contract/candidate registry, versioned rules, account reconciliation and explicit tournament context
2. Payoff-constraint scanner with candidate outcomes, fusion, independent winners, NA/refund and stale-depth tests
3. Evidence registry with provenance, duplicate detection, time/metadata checks and public-source permissions
4. Correlated scenario valuation and terminal-hurdle reporting
5. Order simulator and replay tests for spread, depth, slippage, adverse selection, partial fills and reconnects
6. Configurable position/cluster/orphan-leg limits, idempotency, shared rate limiter, stale-data stop and kill switch
7. Shadow proposals that record expected/robust edge, objective improvement, capacity, worst partial-fill loss and source/rule versions
8. Only after validation: explicitly bounded live execution under the user's existing authorization and any necessary credential setup permissions

**Acceptance tests:** the bot must reject incomplete Massachusetts D/R YES “arbitrage”; distinguish Osborn from Democrats; allow multiple party YES outcomes for fusion; count mirrored book liquidity once; stop on rule changes or stale book revisions; reject missing fees/limits semantics; refrain from treating a poll percentage as win probability; retain national/state correlation; detect conflicting source sample counts; survive lost acknowledgments without duplicate orders; reconcile all fills; respect the deadline and confirm orders are inactive when trading ends.

**Remaining unknowns that block confident deployment:** precise current market predicates outside the checked subset; full chamber definitions; whether contract inventory fully covers each required election; current executable prices/depth; simulated fee details and position limits; order atomicity/fill behavior; valuation-mark formula; reliable accessible live forecast exports and permitted ingestion; calibrated rival terminal wealth; robustness of any directional edge. The evidence supports building and shadow-testing this architecture, not claiming a first-place guarantee.

## Local evidence used

- `execution_valuation_review_2026-10-01.md`: reconciled holdings, prior non-simultaneous depth and API notes
- `structural_scan_2026-10-01_185125.json`: previous rule/candidate checks and negative structural scan
- `public_market_inventory_2026-10-01_184851.json`: 237-contract discovery universe; card prices explicitly non-executable

All quoted holdings and historical book facts are from those prior snapshots. Public sources in this brief were read directly; no paid content or authenticated data was circumvented.
