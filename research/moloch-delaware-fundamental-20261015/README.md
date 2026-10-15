# Delaware Republican NO: a fundamental EV candidate

Research observed October 15, 2026, approximately 23:34–23:52 UTC. No account client, order, VPS operation or strategy activation occurred. Public evidence and reproducible arithmetic are indexed in [evidence.json](evidence.json). Five public HTTPS pages were fingerprinted at 23:51:50Z; these body hashes are retrieval fingerprints, not authenticated account receipts or reproductions of the raw pages.

**Conclusion:** Delaware Republican NO deserves a bounded, explicitly event-holding evaluation. The external forecasts suggest substantial value near 0.90, conditional on their probabilities and the contract's payout mapping. A defensible *empirical* lower probability above 0.90 was not established here. That limits a calibration claim; it does not invalidate an explicitly sized model-based trade or require a frequentist confidence certificate for every decision. Do not silently install 0.995, or an arbitrary haircut to 0.95, as calibrated truth.

## What the sources establish

The [official November 3 ballot](https://elections.delaware.gov/elections/general/sampleballots/GE2026/02-27.html) lists Chris Coons as Democrat and Michael “Dr. Mike” Katz as Republican for U.S. Senator, with a write-in option. These are current nominations, not guarantees about every possible resolution scenario.

The execution owner supplied an original official node receipt, received **23:34:04.457Z**: market386, root node/contract387, exchange1075, race62957, stage98087, Federal U.S. Senate Delaware, General, Party Winner Republican Party, election2026-11-03. Its displayed settlement date is November4 at17:00Z. The retained node contains structured attributes but **no complete resolver text**. The private receipt is not committed; its supplied digest and selected public fields are retained here.

Coons winning under his current Democratic affiliation ordinarily implies Republican NO. The converse is broader: a non-Republican write-in could also produce NO. Exact equivalence is unproved without this market's own affiliation, cancellation/refund, certification, correction and discretionary-update rules. The archived [Kansas contract](../../data/kansas-current-contract-20261004.json) explicitly permits fusion affiliation and discretionary changes; it is evidence of why title matching is insufficient, **not Delaware's resolver**. Do not import its clauses as Delaware facts.

| Primary publisher | Observed forecast | Interpretation |
|---|---|---|
| [Call the Map Delaware](https://www.callthemap.com/senate/delaware) | Oct 15: Coons 99.5%, margin D+31.1; middle 80% margin D+18.2 to D+44.1 | Published model output, not a confidence lower bound. Its historical display mixes issued editions and reconstructed dates. |
| [Who Shows Up Delaware](https://whoshowsup.net/race/senate-de) | Public HTML/noscript dated Oct 15: Coons >99%, margin +26.3; middle 80% +18.3 to +34.3 | Corroborating forecast direction. “>99%” does not identify an exact numeric probability. |
| [Cook's Delaware page](https://www.cookpolitical.com/senate/race/488551) | Solid D; rating unchanged since Feb 11, 2025 | A competitiveness category; no direct conversion to 99% is justified. |

[Call the Map's methodology](https://www.callthemap.com/methodology) reports four held-out cycles2018–2024, six horizons, Senate Student-t uncertainty and residual Democratic bias+2.75points. Its30+point favorite bin reports184 race-horizon rows and zero losses; the same election appears at multiple horizons, so184 is not an independent-election sample. **A provenance inconsistency remains:** the race decomposition describes finance, a25-term fit and a three-source environment including specials/approval; the methodology excludes specials/approval, disables finance and describes a compact Lasso. This may be stale display text, but the published probability was not independently reproduced against the documented calibration.

[Who Shows Up's methods](https://whoshowsup.net/methodology) describe held-year backtests, fundamentals/polls, correlated national/state/regional shocks and fat-tailed errors; Cook and prediction markets are excluded as inputs. Public code is linked. This is not evidence that the two sites' errors are independent: they share election history, polls and national conditions. Neither today's forecasts nor repeated daily readings increase the number of completed test elections.

## An actual historical count, and its limits

Cook's [April2024 archival audit](https://www.cookpolitical.com/analysis/national/cpr-archives/how-accurate-are-cook-political-reports-race-ratings) reports that its only two missed Solid calls were House races. Its available prose does not supply the exact Senate Solid-D denominator. Its final-rating archive also differs from a forecast nineteen days before voting. Therefore this analysis does not manufacture a Senate confidence bound from that headline.

A smaller public cohort is auditable: Cook's [June27,2024 Senate sheet](https://www.cookpolitical.com/sites/default/files/2024-06/2024%20Senate%20AAG%20%2006.27.2024.pdf) labels14 states SolidD. Two are independent incumbents, Maine and Vermont; exclude them from a literal-D-win cohort. The remaining12 are CA,CT,DE,HI,MA,MN,NJ,NM,NY,RI,VA,WA. All12 elected Democratic candidates according to the [official Clerk compilation](https://clerk.house.gov/member_info/electionInfo/2024/statistics2024.pdf). California's simultaneous special/full-term votes count as **one** state-race, not two independent successes. Per-state page references are in the JSON.

For12 successes/12 trials, an exact one-sided95% binomial lower bound is `0.05^(1/12) = 0.7790778081`. For the broader **non-Republican winner** target, include King/Sanders:14/14, bound`0.05^(1/14) = 0.8073638243`. These calculations assume independent, exchangeable trials; states from one national cycle do not establish independence. Selection of a convenient accessible year, June rather than October ratings, rating changes, candidates and shared shocks further limit transfer. Even under the optimistic IID assumption, neither cohort certifies90% reliability. Zero failures would require at least29 independent comparable observations just to move that bound above90%; that is a sample-size calculation, not29 observations found here.

## Price and holding thresholds

Let `pL` be an explicitly chosen conservative probability of ordinary NO payout, `e` an upper allowance for exceptional resolution, `r` its minimum payoff, `b` fees plus execution/capital/model-risk allowance per share, and `c` the actual depth-weighted entry price. Then a conservative price ceiling is:

`cMax = (1-e)*pL + e*r - b`.

This formula requires a *conditional* ordinary-event bound and a separate exception bound; it must not double count an exception already included in `pL`. No empirical `e` bound was found. If an exact rule instead guarantees an entry-price refund on every exception, substitute `r=c` and solve `cMax = pL - b/(1-e)`; that favorable refund assumption is currently unverified.

Illustrative sensitivity only, with worst-case exceptional payout0, `e=1%`, `b=0.01`:

| Assumed ordinary probability | Entry ceiling | Buffered edge at0.900 |
|---|---:|---:|
|0.995 (publisher point estimate)|0.97505|0.07505|
|0.950 (hypothetical haircut)|0.93050|0.03050|
|0.900 (hypothetical stress case)|0.88100|−0.01900|

At assumed `pL=.95`, price `.90` and allowance `.01`, break-even zero-payoff exception probability is `1-.91/.95 = 4.210526%`. This is tolerated risk, **not estimated risk**. At 100 shares, the middle scenario costs 90 and has a buffered modeled surplus 3.05; an upset/zero payout can lose 90. The current 40,000 total/10,000 market caps are ceilings, not evidence supporting that concentration.

A concrete sizing sensitivity is a maximum principal loss of 1% of a 40,000 bank: 400, hence at most 444 shares at `.90` costing 399.60. Ordinary winning profit is 44.40; full loss is 399.60. Under the middle assumptions its modeled gross expectation is 17.982, buffered surplus 13.542. The 1% risk budget is an illustrative policy choice, not an optimized allocation or live instruction. Its downside remains the whole principal, and correlated political/model exposure elsewhere consumes the same risk budget. This small cohort can test the holding process without treating the 10,000 market ceiling as a target.

For existing holdings, compare a justified forward payout bound to an **executable sell bid**, including capital needs and portfolio risk; historical acquisition cost is sunk for this decision and still remains essential for reported profit. The parent-reported60-share sale at0.900 is an actual disposal report, not independently reverified by this research. Under0.995 alone, holding instead has modeled extra expectation5.70 before uncertainty and opportunity cost; this is not a realized loss or guaranteed missed profit. A latest YES print0.100 does not prove an executable NO ask0.900 or its depth.

## Smallest useful next experiment

1. Retain the exact visible Delaware resolver through the execution owner's existing authorized source path; confirm the ordinary payout mapping and mark the unresolved exception exposure explicitly. Obtain the current executable NO ask/bid and quantity depth through the existing worker. No additional account client is needed.
2. Use an explicitly approved forecast-risk floor/exception allowance to produce a **read-only fundamental valuation** for this one contract and log its price ceiling against live executable depth. Label subjective risk tolerances as such; stop admitting that valuation when the source date, ballot, rule or model provenance changes.
3. Evaluate a separate, fully funded holding cohort and an explicit horizon before any activation. The current ordinary60-second maximum-age exit has no demonstrated60-second repricing alpha from these sources and can realize a spread loss long before a terminal edge pays. Election is19calendar days away; displayed settlement is about20days away, while certification/legal delay is unbounded by the node alone. A terminal-value thesis must not promise repeated intraday turnover or borrow against future winnings.

The candidate is a concrete test of fundamental pricing rather than another spread-only model. Remaining decisions are exact payout semantics, an honest model-risk allowance, executable depth and an appropriate holding policy; no validated short-horizon alpha or production profitability is claimed.
